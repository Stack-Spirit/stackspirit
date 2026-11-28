/**
 * Pay-As-You-Go Hetzner compute backed by prepaid Compute Credits.
 *
 * The panel is deliberately split from the Always-On (monthly Contabo) catalog:
 * hourly machines are a different billing model with different guardrails. The
 * only money the panel can move is a Stripe Checkout hand-off after an explicit
 * top-up amount; it never charges a card itself and it never invents a balance or
 * an exchange rate.
 *
 * Endpoints (see the job brief):
 *   GET    /v1/payg/plans
 *   GET    /v1/credits/balance
 *   GET    /v1/payg/instances
 *   POST   /v1/credits/topup
 *   DELETE /v1/payg/instances/:id
 *
 * The bearer session is never stored or rendered here. A host can inject its own
 * `PaygComputeApi` (preferred) or pass a `getAccessToken` callback to
 * `createPaygHttpApi`; the token is only placed in the Authorization header of an
 * authenticated request.
 */

import {
  useCallback,
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
  type FormEvent,
} from "react";

import { useCloudAccount } from "../cloud/store";
import { CLOUD_DEFAULT_API_URL, type CloudFetch } from "../cloud/types";
import { formatAgeParts, formatClock } from "../domain/format";

export type PaygCurrency = "EUR" | "USD";
export type PaygRegion = "EU" | "US" | "SG";
export type PaygLoadStatus = "idle" | "loading" | "ready" | "error";

export interface PaygRegionRate {
  hourly_eur: number | null;
  hourly_usd: number | null;
}

/** One hourly Hetzner tier. Rates already include the +20% StackSpirit markup. */
export interface PaygPlan {
  id: string;
  name: string;
  vcpu: number;
  ram_gb: number;
  disk_gb: number;
  disk_kind: string | null;
  provider_spec: string | null;
  hourly_eur: number | null;
  hourly_usd: number | null;
  regions: PaygRegion[];
  region_rates: Partial<Record<PaygRegion, PaygRegionRate>>;
}

export interface PaygCreditBalance {
  balance_eur: number | null;
  balance_usd: number | null;
  updated_at: string | null;
}

export interface PaygInstance {
  id: string;
  name: string;
  plan_id: string | null;
  plan_name: string | null;
  region: PaygRegion;
  started_at: string | null;
  hourly_eur: number | null;
  hourly_usd: number | null;
}

export interface PaygTopUpResult {
  checkout_url: string;
}

/** The small, explicit surface a host (or a test) can implement. */
export interface PaygComputeApi {
  getPaygPlans(): Promise<PaygPlan[]>;
  getCreditBalance(): Promise<PaygCreditBalance>;
  getPaygInstances(): Promise<PaygInstance[]>;
  topUpCredits(amount: number, currency: PaygCurrency): Promise<PaygTopUpResult>;
  stopPaygInstance(instanceId: string): Promise<void>;
}

export interface PaygComputePanelProps {
  /** Host-injected API. When omitted, a plain `/v1` HTTP transport is used. */
  api?: PaygComputeApi;
  /** Controlled currency, normally shared with the monthly catalog toggle. */
  currency?: PaygCurrency;
  onCurrencyChange?: (currency: PaygCurrency) => void;
  /** Overrides the cloud-store phase for stories and tests. */
  signedIn?: boolean;
  /** Redirect hand-off; defaults to `window.location.assign`. */
  navigate?: (url: string) => void;
  /** Clock injection for deterministic elapsed-time tests. */
  now?: () => number;
  /** True when the panel is rendered inside another cloud panel. */
  embedded?: boolean;
}

interface HourlyRate {
  amount: number;
  currency: PaygCurrency;
  /** True when the selected currency is unpublished and the other is shown. */
  fallback: boolean;
}

const PAYG_REGIONS: readonly PaygRegion[] = ["EU", "US", "SG"];
const MIN_TOP_UP = 5;
const TOP_UP_PRESETS = [5, 10, 25, 50] as const;
const REQUEST_TIMEOUT_MS = 15_000;
const MAX_BODY_CHARS = 1_000_000;
const SAFE_INSTANCE_ID = /^[A-Za-z0-9_-]{1,128}$/;
const PANEL_STYLE = { display: "grid" as const, gap: 14 };

/**
 * Reference tiers used only when `GET /v1/payg/plans` is unavailable. The base
 * rates are Hetzner-style hourly estimates; the displayed rate applies the same
 * +20% StackSpirit markup as the live catalogue. They are never a checkout quote
 * and are labelled as reference data in the UI.
 */
const FALLBACK_TIERS: ReadonlyArray<{
  ram_gb: number;
  vcpu: number;
  disk_gb: number;
  base_hourly_eur: number;
}> = [
  { ram_gb: 4, vcpu: 2, disk_gb: 40, base_hourly_eur: 0.0061 },
  { ram_gb: 8, vcpu: 4, disk_gb: 80, base_hourly_eur: 0.0122 },
  { ram_gb: 16, vcpu: 8, disk_gb: 160, base_hourly_eur: 0.0245 },
  { ram_gb: 32, vcpu: 16, disk_gb: 320, base_hourly_eur: 0.049 },
  { ram_gb: 64, vcpu: 32, disk_gb: 640, base_hourly_eur: 0.098 },
];

export const FALLBACK_PAYG_PLANS: PaygPlan[] = FALLBACK_TIERS.map((tier) => ({
  id: `hetzner-${tier.ram_gb}gb`,
  name: `Hetzner ${tier.ram_gb} GB`,
  vcpu: tier.vcpu,
  ram_gb: tier.ram_gb,
  disk_gb: tier.disk_gb,
  disk_kind: "SSD",
  provider_spec: "Hetzner Cloud · hourly",
  hourly_eur: roundHourly(tier.base_hourly_eur * 1.2),
  hourly_usd: null,
  regions: [...PAYG_REGIONS],
  region_rates: {},
}));

export function PaygComputePanel({
  api: apiProp,
  currency: controlledCurrency,
  onCurrencyChange,
  signedIn: signedInProp,
  navigate,
  now = Date.now,
  embedded = false,
}: PaygComputePanelProps) {
  const accountPhase = useCloudAccount((state) => state.phase);
  const signedIn = signedInProp ?? accountPhase === "signed_in";
  const defaultApi = useMemo(() => createPaygHttpApi(), []);
  const api = apiProp ?? defaultApi;

  const [ownCurrency, setOwnCurrency] = useState<PaygCurrency>("EUR");
  const currency = controlledCurrency ?? ownCurrency;
  const showCurrencyToggle = controlledCurrency === undefined;

  const [plans, setPlans] = useState<PaygPlan[] | null>(null);
  const [plansStatus, setPlansStatus] = useState<PaygLoadStatus>("loading");
  const [plansError, setPlansError] = useState<string | null>(null);

  const [balance, setBalance] = useState<PaygCreditBalance | null>(null);
  const [balanceStatus, setBalanceStatus] = useState<PaygLoadStatus>("loading");
  const [balanceError, setBalanceError] = useState<string | null>(null);

  const [instances, setInstances] = useState<PaygInstance[]>([]);
  const [instancesStatus, setInstancesStatus] = useState<PaygLoadStatus>("loading");
  const [instancesError, setInstancesError] = useState<string | null>(null);

  const [region, setRegion] = useState<PaygRegion>("EU");
  const [customAmount, setCustomAmount] = useState("");
  const [topUpBusy, setTopUpBusy] = useState(false);
  const [topUpError, setTopUpError] = useState<string | null>(null);
  const [topUpNotice, setTopUpNotice] = useState<string | null>(null);

  const [confirmingId, setConfirmingId] = useState<string | null>(null);
  const [stoppingId, setStoppingId] = useState<string | null>(null);
  const [stopError, setStopError] = useState<{ id: string; message: string } | null>(null);
  const [stopNotice, setStopNotice] = useState<string | null>(null);

  const nowRef = useRef(now);
  const [nowMs, setNowMs] = useState(() => now());
  const customAmountId = useId();
  const topUpErrorId = useId();

  useEffect(() => {
    nowRef.current = now;
  }, [now]);

  const loadPlans = useCallback(async () => {
    setPlansStatus("loading");
    setPlansError(null);
    try {
      const next = await api.getPaygPlans();
      setPlans(next);
      setPlansStatus("ready");
    } catch (error) {
      setPlansError(messageOf(error));
      setPlansStatus("error");
    }
  }, [api]);

  const loadBalance = useCallback(async () => {
    if (!signedIn) return;
    setBalanceStatus("loading");
    setBalanceError(null);
    try {
      const next = await api.getCreditBalance();
      setBalance(next);
      setBalanceStatus("ready");
    } catch (error) {
      setBalanceError(messageOf(error));
      setBalanceStatus("error");
    }
  }, [api, signedIn]);

  const loadInstances = useCallback(async () => {
    if (!signedIn) return;
    setInstancesStatus("loading");
    setInstancesError(null);
    try {
      const next = await api.getPaygInstances();
      setInstances(next);
      setNowMs(nowRef.current());
      setInstancesStatus("ready");
    } catch (error) {
      setInstancesError(messageOf(error));
      setInstancesStatus("error");
    }
  }, [api, signedIn]);

  useEffect(() => {
    let active = true;
    api
      .getPaygPlans()
      .then((next) => {
        if (!active) return;
        setPlans(next);
        setPlansStatus("ready");
        setPlansError(null);
      })
      .catch((error: unknown) => {
        if (!active) return;
        setPlansError(messageOf(error));
        setPlansStatus("error");
      });
    return () => {
      active = false;
    };
  }, [api]);

  useEffect(() => {
    if (!signedIn) return;
    let active = true;
    api
      .getCreditBalance()
      .then((next) => {
        if (!active) return;
        setBalance(next);
        setBalanceStatus("ready");
        setBalanceError(null);
      })
      .catch((error: unknown) => {
        if (!active) return;
        setBalanceError(messageOf(error));
        setBalanceStatus("error");
      });
    api
      .getPaygInstances()
      .then((next) => {
        if (!active) return;
        setInstances(next);
        setNowMs(nowRef.current());
        setInstancesStatus("ready");
        setInstancesError(null);
      })
      .catch((error: unknown) => {
        if (!active) return;
        setInstancesError(messageOf(error));
        setInstancesStatus("error");
      });
    return () => {
      active = false;
    };
  }, [api, signedIn]);

  useEffect(() => {
    if (instances.length === 0) return;
    const timer = window.setInterval(() => {
      setNowMs(nowRef.current());
    }, 1_000);
    return () => window.clearInterval(timer);
  }, [instances.length]);

  function chooseCurrency(next: PaygCurrency) {
    if (showCurrencyToggle) setOwnCurrency(next);
    onCurrencyChange?.(next);
  }

  async function startTopUp(amount: number) {
    if (!signedIn) {
      setTopUpError("Sign in to StackSpirit to add Compute Credits.");
      return;
    }
    if (!Number.isFinite(amount) || amount < MIN_TOP_UP) {
      setTopUpError(`Enter at least ${formatTopUpAmount(MIN_TOP_UP, currency)}.`);
      return;
    }
    setTopUpBusy(true);
    setTopUpError(null);
    setTopUpNotice(null);
    try {
      const result = await api.topUpCredits(roundHourly(amount), currency);
      const checkoutUrl = safeCheckoutUrl(result.checkout_url);
      if (!checkoutUrl) {
        throw new Error(
          "The Compute Credits API did not return a safe Stripe Checkout link. Nothing was charged.",
        );
      }
      setTopUpNotice(`Opening Stripe Checkout for ${formatTopUpAmount(amount, currency)}…`);
      setTopUpBusy(false);
      (navigate ?? defaultNavigate)(checkoutUrl);
    } catch (error) {
      setTopUpBusy(false);
      setTopUpNotice(null);
      setTopUpError(messageOf(error));
    }
  }

  function submitCustomAmount(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const value = customAmount.trim();
    const amount = Number(value);
    if (!value || !Number.isFinite(amount) || amount < MIN_TOP_UP) {
      setTopUpError(`Enter a custom amount of at least ${formatTopUpAmount(MIN_TOP_UP, currency)}.`);
      return;
    }
    void startTopUp(amount);
  }

  async function stopInstance(instance: PaygInstance) {
    setStoppingId(instance.id);
    setStopError(null);
    setStopNotice(null);
    try {
      await api.stopPaygInstance(instance.id);
      setInstances((current) => current.filter((candidate) => candidate.id !== instance.id));
      setConfirmingId(null);
      setStopNotice(`${instance.name} was stopped and deleted. Billing stopped immediately.`);
      void loadBalance();
    } catch (error) {
      setStopError({ id: instance.id, message: messageOf(error) });
    } finally {
      setStoppingId(null);
    }
  }

  const displayedPlans =
    plans ?? (plansStatus === "error" ? FALLBACK_PAYG_PLANS : []);
  const visiblePlans = displayedPlans.filter((plan) => plan.regions.includes(region));
  const selectedBalance = balanceIn(balance, currency);
  const otherCurrency: PaygCurrency = currency === "EUR" ? "USD" : "EUR";
  const otherBalance = balanceIn(balance, otherCurrency);
  const shownBalance = selectedBalance ?? otherBalance;
  const shownCurrency = selectedBalance !== null ? currency : otherCurrency;
  const rateCatalog = plans ?? (plansStatus === "error" ? FALLBACK_PAYG_PLANS : null);

  return (
    <section
      className={embedded ? undefined : "nx-cloud nx-cloud--plans"}
      style={embedded ? PANEL_STYLE : undefined}
      data-testid="payg-compute-panel"
      aria-label="Pay-As-You-Go compute"
    >
      {!embedded ? (
        <div className="nx-cloud__title">
          <div>
            <h3>Pay-As-You-Go Compute</h3>
            <p>
              Hetzner machines billed by the hour from prepaid Compute Credits. No monthly
              minimum; stop and delete a machine and billing stops immediately.
            </p>
          </div>
          <div className="nx-cloud__currency">
            {showCurrencyToggle ? (
              <div
                className="nx-segment nx-segment--tiny"
                role="group"
                aria-label="Compute Credit currency"
              >
                <button
                  type="button"
                  aria-pressed={currency === "EUR"}
                  onClick={() => chooseCurrency("EUR")}
                  data-testid="payg-currency-eur"
                >
                  EUR
                </button>
                <button
                  type="button"
                  aria-pressed={currency === "USD"}
                  onClick={() => chooseCurrency("USD")}
                  data-testid="payg-currency-usd"
                >
                  USD
                </button>
              </div>
            ) : null}
            <span className="nx-chip nx-chip--muted">Hourly · {currency}</span>
          </div>
        </div>
      ) : null}

      <div className="nx-cloud__provisioning" data-testid="payg-credits">
        <div className="nx-cloud__provisioning-head">
          <h4>Compute Credits</h4>
          <span className="nx-chip nx-chip--muted">Prepaid</span>
        </div>

        {!signedIn ? (
          <p className="nx-cloud__meta" data-testid="payg-sign-in">
            Sign in to StackSpirit to see your Compute Credit balance and add credits.
            Hourly rates below are public.
          </p>
        ) : balanceStatus === "loading" && !balance ? (
          <p className="nx-cloud__meta" role="status" data-testid="payg-balance-loading">
            Checking your Compute Credit balance…
          </p>
        ) : balanceStatus === "error" && !balance ? (
          <div className="nx-cloud__error" role="alert" data-testid="payg-balance-error">
            <p>{balanceError ?? "Your Compute Credit balance could not be read."}</p>
            <button
              type="button"
              className="nx-btn nx-btn--quiet nx-btn--sm"
              onClick={() => void loadBalance()}
            >
              Retry
            </button>
          </div>
        ) : balance ? (
          <>
            <div
              className="nx-cloud__plan-head"
              data-testid="payg-balance"
              data-currency={shownCurrency}
            >
              <strong className="nx-cloud__price">
                {shownBalance !== null
                  ? formatMoney(shownBalance, shownCurrency)
                  : "Balance unavailable"}
              </strong>
              <span className="nx-chip nx-chip--server">Available</span>
            </div>
            <span className="nx-cloud__meta" data-testid="payg-balance-secondary">
              {selectedBalance !== null
                ? otherBalance !== null
                  ? `${otherCurrency} balance ${formatMoney(otherBalance, otherCurrency)}.`
                  : `${otherCurrency} balance is not published.`
                : otherBalance !== null
                  ? `${currency} balance is not published; showing ${otherCurrency}.`
                  : "No Compute Credit balance is published for this account."}
            </span>
            {balance.updated_at ? (
              <span className="nx-cloud__meta">
                Updated {formatClock(balance.updated_at)}
              </span>
            ) : null}
            {balanceStatus === "error" ? (
              <p className="nx-cloud__error" role="alert">
                {balanceError ?? "The balance could not be refreshed."}
              </p>
            ) : null}
          </>
        ) : (
          <p className="nx-cloud__meta" data-testid="payg-balance-empty">
            No Compute Credit balance is available yet.
          </p>
        )}

        {signedIn ? (
          <div style={PANEL_STYLE} data-testid="payg-topup">
            <div
              className="nx-cloud__actions"
              role="group"
              aria-label="Quick Compute Credit top-ups"
            >
              {TOP_UP_PRESETS.map((value) => (
                <button
                  key={value}
                  type="button"
                  className="nx-btn nx-btn--quiet nx-btn--sm"
                  disabled={topUpBusy}
                  onClick={() => void startTopUp(value)}
                  data-testid={`payg-topup-preset-${value}`}
                >
                  {formatTopUpAmount(value, currency)}
                </button>
              ))}
            </div>

            <form
              onSubmit={submitCustomAmount}
              noValidate
              style={{ display: "flex", alignItems: "flex-end", flexWrap: "wrap", gap: 8 }}
            >
              <label className="nx-field" htmlFor={customAmountId} style={{ maxWidth: 190 }}>
                Custom amount ({currency})
                <input
                  id={customAmountId}
                  type="number"
                  min={MIN_TOP_UP}
                  step="0.01"
                  inputMode="decimal"
                  value={customAmount}
                  disabled={topUpBusy}
                  aria-invalid={topUpError !== null}
                  aria-describedby={topUpError ? topUpErrorId : undefined}
                  onChange={(event) => setCustomAmount(event.currentTarget.value)}
                  data-testid="payg-topup-custom"
                />
              </label>
              <button
                type="submit"
                className="nx-btn nx-btn--sm"
                disabled={topUpBusy}
                data-testid="payg-topup-submit"
              >
                {topUpBusy ? "Opening…" : "Add credits"}
              </button>
            </form>

            <span className="nx-cloud__meta">
              Minimum {formatTopUpAmount(MIN_TOP_UP, currency)}. Credits never expire; they
              are only spent while a machine is running.
            </span>

            {topUpNotice ? (
              <p className="nx-cloud__meta" role="status" data-testid="payg-topup-notice">
                {topUpNotice}
              </p>
            ) : null}
            {topUpError ? (
              <p
                className="nx-cloud__error"
                role="alert"
                id={topUpErrorId}
                data-testid="payg-topup-error"
              >
                {topUpError}
              </p>
            ) : null}
          </div>
        ) : null}
      </div>

      <div className="nx-cloud__provisioning" data-testid="payg-plans-section">
        <div className="nx-cloud__provisioning-head">
          <h4>Hetzner hourly tiers</h4>
          <span className="nx-chip nx-chip--muted">+20% included</span>
        </div>

        <div className="nx-cloud__actions" style={{ justifyContent: "space-between" }}>
          <div
            className="nx-segment nx-segment--tiny"
            role="group"
            aria-label="Pay-As-You-Go region"
          >
            {PAYG_REGIONS.map((value) => (
              <button
                key={value}
                type="button"
                aria-pressed={region === value}
                onClick={() => setRegion(value)}
                data-testid={`payg-region-${value.toLowerCase()}`}
              >
                {value}
              </button>
            ))}
          </div>
          <button
            type="button"
            className="nx-btn nx-btn--quiet nx-btn--sm"
            disabled={plansStatus === "loading"}
            onClick={() => void loadPlans()}
            data-testid="payg-plans-refresh"
          >
            Refresh rates
          </button>
        </div>

        {plansStatus === "loading" && !plans ? (
          <p className="nx-cloud__meta" role="status" data-testid="payg-plans-loading">
            Loading Pay-As-You-Go rates…
          </p>
        ) : null}

        {plansStatus === "error" ? (
          <div className="nx-cloud__error" role="alert" data-testid="payg-plans-fallback">
            <p>
              {plansError ?? "Live Pay-As-You-Go rates are unavailable."} Showing
              reference Hetzner tiers with the +20% StackSpirit rate; retry before you
              rely on them.
            </p>
          </div>
        ) : null}

        {currency === "USD" &&
        visiblePlans.some((plan) => planRate(plan, region, currency)?.fallback) ? (
          <p className="nx-cloud__meta" role="status" data-testid="payg-rate-fallback">
            USD hourly rates are not published for every tier. Those cards show EUR;
            StackSpirit never guesses a currency conversion.
          </p>
        ) : null}

        {plansStatus === "loading" && !plans ? null : visiblePlans.length === 0 ? (
          <p className="nx-cloud__empty" data-testid="payg-plans-empty">
            {plansStatus === "error"
              ? "No reference tiers are available."
              : `No Pay-As-You-Go tiers are published for ${region} right now.`}
          </p>
        ) : (
          <ul
            className="nx-cloud__plans"
            data-testid="payg-plans"
            aria-label={`Pay-As-You-Go Hetzner tiers for ${region}`}
          >
            {visiblePlans.map((plan) => (
              <PlanCard
                key={plan.id}
                plan={plan}
                region={region}
                currency={currency}
                balance={balance}
                signedIn={signedIn}
              />
            ))}
          </ul>
        )}
      </div>

      <div className="nx-cloud__provisioning" data-testid="payg-instances-section">
        <div className="nx-cloud__provisioning-head">
          <h4>Active Pay-As-You-Go machines</h4>
          {signedIn ? (
            <button
              type="button"
              className="nx-btn nx-btn--quiet nx-btn--sm"
              disabled={instancesStatus === "loading"}
              onClick={() => void loadInstances()}
              data-testid="payg-instances-refresh"
            >
              Refresh
            </button>
          ) : null}
        </div>

        {!signedIn ? (
          <p className="nx-cloud__meta" data-testid="payg-instances-sign-in">
            Sign in to StackSpirit to see and stop your Pay-As-You-Go machines.
          </p>
        ) : instancesStatus === "loading" && instances.length === 0 ? (
          <p className="nx-cloud__meta" role="status" data-testid="payg-instances-loading">
            Checking for running Pay-As-You-Go machines…
          </p>
        ) : instancesStatus === "error" && instances.length === 0 ? (
          <div className="nx-cloud__error" role="alert" data-testid="payg-instances-error">
            <p>{instancesError ?? "Your Pay-As-You-Go machines could not be read."}</p>
            <button
              type="button"
              className="nx-btn nx-btn--quiet nx-btn--sm"
              onClick={() => void loadInstances()}
            >
              Retry
            </button>
          </div>
        ) : instances.length === 0 ? (
          <p className="nx-cloud__empty" data-testid="payg-instances-empty">
            No Pay-As-You-Go machines are running. Credits are only spent while a machine
            runs.
          </p>
        ) : (
          <>
            {instancesStatus === "error" ? (
              <p className="nx-cloud__error" role="alert">
                {instancesError ?? "The machine list could not be refreshed."}
              </p>
            ) : null}
            <ul
              className="nx-cloud__machine-list"
              data-testid="payg-instances"
              aria-label="Active Pay-As-You-Go machines"
            >
              {instances.map((instance) => (
                <InstanceCard
                  key={instance.id}
                  instance={instance}
                  rate={instanceRate(instance, rateCatalog, currency)}
                  nowMs={nowMs}
                  confirming={confirmingId === instance.id}
                  stopping={stoppingId === instance.id}
                  stopError={stopError?.id === instance.id ? stopError.message : null}
                  onAskStop={() => {
                    setConfirmingId(instance.id);
                    setStopError(null);
                    setStopNotice(null);
                  }}
                  onCancelStop={() => {
                    setConfirmingId(null);
                    setStopError(null);
                  }}
                  onConfirmStop={() => void stopInstance(instance)}
                />
              ))}
            </ul>
          </>
        )}

        {signedIn && stopNotice ? (
          <p
            className="nx-cloud__relay nx-cloud__relay--ok"
            role="status"
            data-testid="payg-stop-notice"
          >
            {stopNotice}
          </p>
        ) : null}
        {signedIn && stopError && !confirmingId ? (
          <p className="nx-cloud__error" role="alert" data-testid="payg-stop-error">
            {stopError.message}
          </p>
        ) : null}
      </div>

      <aside
        className="nx-cloud__relay nx-cloud__relay--ok"
        data-testid="payg-guardrail"
        aria-label="Zero-surprise billing guardrail"
        style={{ display: "grid", gap: 4 }}
      >
        <strong>Zero-surprise guardrail</strong>
        <span>
          We email you when a running machine has less than 2 hours of Compute Credit
          remaining. At a €0 balance (or $0 when you pay in USD), the machine shuts down
          automatically, so an unattended machine cannot build up a surprise bill.
        </span>
      </aside>
    </section>
  );
}

function PlanCard({
  plan,
  region,
  currency,
  balance,
  signedIn,
}: {
  plan: PaygPlan;
  region: PaygRegion;
  currency: PaygCurrency;
  balance: PaygCreditBalance | null;
  signedIn: boolean;
}) {
  const rate = planRate(plan, region, currency);
  const hours = runtimeFromBalance(balance, rate);
  const runtime = !signedIn
    ? "Sign in to estimate runtime"
    : !rate
      ? "Rate unavailable"
      : hours === null
        ? `No ${rate.currency} credit balance to estimate from`
        : hours <= 0
          ? "Top up to start"
          : `≈ ${formatRuntimeHours(hours)} from your balance`;

  return (
    <li className="nx-cloud__plan" data-testid={`payg-plan-${plan.id}`}>
      <div className="nx-cloud__plan-head">
        <strong>{plan.name}</strong>
        <span
          className="nx-cloud__price"
          data-testid={`payg-rate-${plan.id}`}
          data-currency={rate?.currency ?? currency}
          title="Hourly rate including the +20% StackSpirit markup"
        >
          {rate ? formatHourlyRate(rate) : `${currency} rate unavailable`}
        </span>
      </div>
      <span className="nx-cloud__meta" data-testid={`payg-specs-${plan.id}`}>
        {planSpecs(plan)}
      </span>
      {plan.provider_spec ? (
        <span className="nx-cloud__meta">{plan.provider_spec}</span>
      ) : null}
      <span className="nx-cloud__meta" data-testid={`payg-runtime-${plan.id}`}>
        {runtime}
      </span>
    </li>
  );
}

function InstanceCard({
  instance,
  rate,
  nowMs,
  confirming,
  stopping,
  stopError,
  onAskStop,
  onCancelStop,
  onConfirmStop,
}: {
  instance: PaygInstance;
  rate: HourlyRate | null;
  nowMs: number;
  confirming: boolean;
  stopping: boolean;
  stopError: string | null;
  onAskStop: () => void;
  onCancelStop: () => void;
  onConfirmStop: () => void;
}) {
  const seconds = elapsedSeconds(instance.started_at, nowMs);
  const elapsed = seconds === null ? "elapsed unavailable" : formatAgeParts(seconds);
  const details = [instance.plan_name, instance.region].filter(Boolean).join(" · ");
  const started = instance.started_at
    ? `Started ${formatClock(instance.started_at)}`
    : "Start time unavailable";

  return (
    <li className="nx-cloud__machine" data-testid={`payg-instance-${instance.id}`}>
      <div className="nx-cloud__machine-main">
        <span className="nx-cloud__presence nx-cloud__presence--ok" aria-hidden="true" />
        <span style={{ display: "grid", gap: 2, minWidth: 0 }}>
          <span className="nx-cloud__machine-name">{instance.name}</span>
          <span className="nx-cloud__meta">
            {details ? `${details} · ` : ""}
            {started}
          </span>
        </span>
      </div>
      <div className="nx-cloud__machine-actions">
        <span className="nx-chip nx-chip--server" data-testid={`payg-elapsed-${instance.id}`}>
          Running {elapsed}
        </span>
        <span className="nx-cloud__price" data-testid={`payg-burn-${instance.id}`}>
          {rate ? formatHourlyRate(rate) : "Rate unavailable"}
        </span>
        <button
          type="button"
          className="nx-btn nx-btn--quiet nx-btn--sm"
          disabled={stopping}
          onClick={onAskStop}
          data-testid={`payg-stop-${instance.id}`}
        >
          Stop &amp; Delete
        </button>
      </div>

      {confirming ? (
        <div
          className="nx-cloud__device"
          role="alertdialog"
          aria-label={`Stop and delete ${instance.name}`}
          data-testid={`payg-stop-confirm-${instance.id}`}
          style={{ flexBasis: "100%", display: "grid", gap: 8 }}
        >
          <p>
            <strong>Billing stops immediately.</strong> {instance.name} is deleted now;
            no further hourly charges accrue. This cannot be undone.
          </p>
          <div className="nx-cloud__actions">
            <button
              type="button"
              className="nx-btn nx-btn--sm"
              disabled={stopping}
              onClick={onConfirmStop}
              data-testid={`payg-confirm-stop-${instance.id}`}
              autoFocus
            >
              {stopping ? "Stopping…" : "Stop & Delete"}
            </button>
            <button
              type="button"
              className="nx-btn nx-btn--quiet nx-btn--sm"
              disabled={stopping}
              onClick={onCancelStop}
              data-testid={`payg-cancel-stop-${instance.id}`}
            >
              Keep running
            </button>
          </div>
          {stopError ? (
            <p
              className="nx-cloud__error"
              role="alert"
              data-testid={`payg-stop-error-${instance.id}`}
            >
              {stopError}
            </p>
          ) : null}
        </div>
      ) : null}
    </li>
  );
}

/** The plain HTTP transport used when a host does not inject its own API. */
export interface PaygHttpApiOptions {
  apiBaseUrl?: string;
  fetchImpl?: CloudFetch;
  /** Returns the in-memory bearer token; it is never stored by this module. */
  getAccessToken?: () => string | null;
  timeoutMs?: number;
}

export function createPaygHttpApi(options: PaygHttpApiOptions = {}): PaygComputeApi {
  const base = normalizeApiBase(options.apiBaseUrl ?? CLOUD_DEFAULT_API_URL);
  const fetchImpl: CloudFetch =
    options.fetchImpl ?? ((input, init) => fetch(input, init));
  const getAccessToken = options.getAccessToken ?? (() => null);
  const timeoutMs = options.timeoutMs ?? REQUEST_TIMEOUT_MS;

  async function request(
    path: string,
    init: {
      method: "GET" | "POST" | "DELETE";
      body?: unknown;
      authenticated?: boolean;
    },
  ): Promise<unknown> {
    const headers: Record<string, string> = { Accept: "application/json" };
    if (init.body !== undefined) headers["Content-Type"] = "application/json";
    if (init.authenticated !== false) {
      let token: string | null = null;
      try {
        token = getAccessToken();
      } catch {
        token = null;
      }
      if (!token) {
        throw new PaygRequestError("Sign in to StackSpirit to use Compute Credits.");
      }
      headers.Authorization = `Bearer ${token}`;
    }

    const controller = new AbortController();
    const timeout = setTimeout(() => controller.abort(), timeoutMs);
    let response: Response;
    try {
      response = await fetchImpl(`${base}${path}`, {
        method: init.method,
        headers,
        body: init.body === undefined ? undefined : JSON.stringify(init.body),
        signal: controller.signal,
        credentials: "omit",
        cache: "no-store",
        redirect: "error",
      });
    } catch (error) {
      if (controller.signal.aborted) {
        throw new PaygRequestError(
          "The Compute Credits service did not answer in time. Retry in a moment.",
        );
      }
      if (error instanceof PaygRequestError) throw error;
      throw new PaygRequestError(
        "The Compute Credits service could not be reached. Check the network and retry.",
      );
    } finally {
      clearTimeout(timeout);
    }

    let text = "";
    try {
      text = await response.text();
    } catch {
      throw new PaygRequestError("The Compute Credits service returned an unreadable answer.");
    }
    if (text.length > MAX_BODY_CHARS) {
      throw new PaygRequestError("The Compute Credits service returned an oversized document.");
    }

    let value: unknown = null;
    if (text.trim()) {
      try {
        value = JSON.parse(text) as unknown;
      } catch {
        if (response.ok) {
          throw new PaygRequestError(
            "The Compute Credits service returned an unreadable document.",
          );
        }
      }
    }

    if (!response.ok) {
      if (response.status === 401) {
        throw new PaygRequestError("Sign in to StackSpirit again.");
      }
      throw new PaygRequestError(
        apiMessage(value) ?? `The Compute Credits service answered with HTTP ${response.status}.`,
      );
    }
    return value;
  }

  return {
    async getPaygPlans() {
      return parsePlans(await request("/v1/payg/plans", { method: "GET", authenticated: false }));
    },

    async getCreditBalance() {
      return parseBalance(
        await request("/v1/credits/balance", { method: "GET", authenticated: true }),
      );
    },

    async getPaygInstances() {
      return parseInstances(
        await request("/v1/payg/instances", { method: "GET", authenticated: true }),
      );
    },

    async topUpCredits(amount, currency) {
      return parseTopUp(
        await request("/v1/credits/topup", {
          method: "POST",
          authenticated: true,
          body: { amount: roundHourly(amount), currency },
        }),
      );
    },

    async stopPaygInstance(instanceId) {
      const id = instanceId.trim();
      if (!SAFE_INSTANCE_ID.test(id)) {
        throw new PaygRequestError("That Pay-As-You-Go machine id is not valid.");
      }
      await request(`/v1/payg/instances/${encodeURIComponent(id)}`, {
        method: "DELETE",
        authenticated: true,
      });
    },
  };
}

class PaygRequestError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "PaygRequestError";
  }
}

function defaultNavigate(url: string): void {
  window.location.assign(url);
}

function normalizeApiBase(raw: string): string {
  const trimmed = raw.trim().replace(/\/+$/, "");
  let url: URL;
  try {
    url = new URL(trimmed);
  } catch {
    throw new Error("The Compute Credits API address is not a valid URL.");
  }
  const local =
    url.hostname === "127.0.0.1" || url.hostname === "localhost" || url.hostname === "::1";
  if (url.protocol !== "https:" && !(url.protocol === "http:" && local)) {
    throw new Error("The Compute Credits API must use https.");
  }
  if (url.username || url.password || url.search || url.hash) {
    throw new Error(
      "The Compute Credits API address cannot contain credentials, a query or a fragment.",
    );
  }
  return trimmed;
}

function safeCheckoutUrl(raw: string): string | null {
  try {
    const url = new URL(raw.trim());
    const local =
      url.hostname === "127.0.0.1" || url.hostname === "localhost" || url.hostname === "::1";
    if (url.protocol !== "https:" && !(url.protocol === "http:" && local)) return null;
    if (url.username || url.password) return null;
    return url.toString();
  } catch {
    return null;
  }
}

function parsePlans(value: unknown): PaygPlan[] {
  const items = Array.isArray(value)
    ? value
    : isRecord(value) && Array.isArray(value.plans)
      ? value.plans
      : isRecord(value) && Array.isArray(value.tiers)
        ? value.tiers
        : isRecord(value) && Array.isArray(value.items)
          ? value.items
          : null;
  if (!items) {
    throw new PaygRequestError(
      "The Compute Credits API returned an unreadable Pay-As-You-Go catalog.",
    );
  }
  return items
    .map(parsePlan)
    .filter((plan): plan is PaygPlan => plan !== null)
    .sort((left, right) => left.ram_gb - right.ram_gb);
}

function parsePlan(value: unknown): PaygPlan | null {
  if (!isRecord(value)) return null;
  const ram = asNumber(value.ram_gb ?? value.memory_gb ?? value.ram);
  if (ram === null) return null;
  const ramGb = Math.round(ram);
  if (ramGb < 4 || ramGb > 64) return null;

  const regionInfo = readRegionInfo(value);
  const id =
    stringField(value.id) ?? stringField(value.slug) ?? `hetzner-${ramGb}gb`;
  return {
    id,
    name: stringField(value.name) ?? `Hetzner ${ramGb} GB`,
    vcpu: Math.round(asNumber(value.vcpu ?? value.vcpus ?? value.cpu) ?? 0),
    ram_gb: ramGb,
    disk_gb: Math.round(asNumber(value.disk_gb ?? value.storage_gb ?? value.disk) ?? 0),
    disk_kind: stringField(value.disk_kind ?? value.storage_type),
    provider_spec: stringField(value.provider_spec ?? value.provider),
    hourly_eur:
      hourlyMoney(value, "eur") ?? firstRegionRate(regionInfo.rates, "eur"),
    hourly_usd:
      hourlyMoney(value, "usd") ?? firstRegionRate(regionInfo.rates, "usd"),
    regions: regionInfo.regions,
    region_rates: regionInfo.rates,
  };
}

function readRegionInfo(value: Record<string, unknown>): {
  regions: PaygRegion[];
  rates: Partial<Record<PaygRegion, PaygRegionRate>>;
} {
  const regions: PaygRegion[] = [];
  const rates: Partial<Record<PaygRegion, PaygRegionRate>> = {};

  function add(region: PaygRegion, rate: PaygRegionRate) {
    if (!regions.includes(region)) regions.push(region);
    rates[region] = rate;
  }

  const rawRegions = value.regions ?? value.available_regions;
  if (Array.isArray(rawRegions)) {
    for (const entry of rawRegions) {
      if (typeof entry === "string") {
        const region = normalizeRegion(entry);
        if (region) add(region, { hourly_eur: null, hourly_usd: null });
        continue;
      }
      if (!isRecord(entry)) continue;
      const region = normalizeRegion(
        entry.code ?? entry.id ?? entry.region ?? entry.name,
      );
      if (!region) continue;
      add(region, {
        hourly_eur: hourlyMoney(entry, "eur") ?? asNumber(entry.eur ?? entry.EUR),
        hourly_usd: hourlyMoney(entry, "usd") ?? asNumber(entry.usd ?? entry.USD),
      });
    }
  } else if (isRecord(rawRegions)) {
    for (const key of Object.keys(rawRegions)) {
      const region = normalizeRegion(key);
      if (region) add(region, { hourly_eur: null, hourly_usd: null });
    }
  }

  const rateMap = isRecord(value.region_rates)
    ? value.region_rates
    : isRecord(value.hourly_by_region)
      ? value.hourly_by_region
      : null;
  if (rateMap) {
    for (const [key, rawRate] of Object.entries(rateMap)) {
      const region = normalizeRegion(key);
      if (!region) continue;
      if (isRecord(rawRate)) {
        add(region, {
          hourly_eur: asNumber(rawRate.eur ?? rawRate.EUR) ?? hourlyMoney(rawRate, "eur"),
          hourly_usd: asNumber(rawRate.usd ?? rawRate.USD) ?? hourlyMoney(rawRate, "usd"),
        });
        continue;
      }
      const rate = asNumber(rawRate);
      if (rate !== null) add(region, { hourly_eur: rate, hourly_usd: null });
    }
  }

  return {
    regions: regions.length > 0 ? regions : [...PAYG_REGIONS],
    rates,
  };
}

function parseBalance(value: unknown): PaygCreditBalance {
  if (!isRecord(value)) {
    throw new PaygRequestError(
      "The Compute Credits API returned an unreadable Compute Credit balance.",
    );
  }
  const balance: PaygCreditBalance = {
    balance_eur: balanceMoney(value, "eur"),
    balance_usd: balanceMoney(value, "usd"),
    updated_at: timestampField(value.updated_at ?? value.fetched_at ?? value.as_of),
  };
  if (balance.balance_eur === null && balance.balance_usd === null) {
    throw new PaygRequestError(
      "The Compute Credits API returned an unreadable Compute Credit balance.",
    );
  }
  return balance;
}

function parseInstances(value: unknown): PaygInstance[] {
  const items = Array.isArray(value)
    ? value
    : isRecord(value) && Array.isArray(value.instances)
      ? value.instances
      : isRecord(value) && Array.isArray(value.items)
        ? value.items
        : null;
  if (!items) {
    throw new PaygRequestError(
      "The Compute Credits API returned an unreadable Pay-As-You-Go machine list.",
    );
  }
  return items
    .map(parseInstance)
    .filter((instance): instance is PaygInstance => instance !== null)
    .sort((left, right) =>
      (left.started_at ?? "").localeCompare(right.started_at ?? ""),
    );
}

function parseInstance(value: unknown): PaygInstance | null {
  if (!isRecord(value)) return null;
  const id =
    stringField(value.id) ??
    stringField(value.instance_id) ??
    stringField(value.machine_id);
  if (!id) return null;
  const status = stringField(value.status)?.toLowerCase();
  if (status && ["deleted", "stopped", "terminated"].includes(status)) return null;

  const plan = isRecord(value.plan) ? value.plan : null;
  const planId =
    stringField(value.plan_id) ?? (plan ? stringField(plan.id) : null);
  const planName =
    stringField(value.plan_name) ?? (plan ? stringField(plan.name) : null);
  const region =
    normalizeRegion(value.region ?? value.location ?? value.zone ?? plan?.region) ?? "EU";

  return {
    id,
    name: stringField(value.name) ?? planName ?? "Pay-As-You-Go machine",
    plan_id: planId,
    plan_name: planName,
    region,
    started_at: timestampField(
      value.started_at ?? value.created_at ?? value.launched_at,
    ),
    hourly_eur:
      hourlyMoney(value, "eur") ?? (plan ? hourlyMoney(plan, "eur") : null),
    hourly_usd:
      hourlyMoney(value, "usd") ?? (plan ? hourlyMoney(plan, "usd") : null),
  };
}

function parseTopUp(value: unknown): PaygTopUpResult {
  if (typeof value === "string" && value.trim() !== "") {
    return { checkout_url: value.trim() };
  }
  if (!isRecord(value)) {
    throw new PaygRequestError(
      "The Compute Credits API did not return a Stripe Checkout link.",
    );
  }
  const direct =
    stringField(value.checkout_url) ??
    stringField(value.stripe_checkout_url) ??
    stringField(value.url);
  if (direct) return { checkout_url: direct };

  for (const key of ["checkout", "session"] as const) {
    const nested = value[key];
    if (!isRecord(nested)) continue;
    const url =
      stringField(nested.url) ?? stringField(nested.checkout_url);
    if (url) return { checkout_url: url };
  }

  throw new PaygRequestError(
    "The Compute Credits API did not return a Stripe Checkout link.",
  );
}

function balanceMoney(value: Record<string, unknown>, unit: "eur" | "usd"): number | null {
  const upper = unit.toUpperCase();
  const directKeys = [
    `balance_${unit}`,
    `credit_balance_${unit}`,
    `available_${unit}`,
    `credits_${unit}`,
  ];
  for (const key of directKeys) {
    const amount = asNumber(value[key]);
    if (amount !== null) return amount;
    const cents = asNumber(value[`${key}_cents`]);
    if (cents !== null) return cents / 100;
  }

  for (const key of ["balance", "credit_balance", "credits", "available"] as const) {
    const container = value[key];
    if (!isRecord(container)) continue;
    const containerCurrency = stringField(container.currency)?.toUpperCase();
    const amount = asNumber(container[unit] ?? container[upper]);
    if (amount !== null) return amount;
    const cents = asNumber(container[`${unit}_cents`]);
    if (cents !== null) return cents / 100;
    if (!containerCurrency || containerCurrency === upper) {
      const bare = asNumber(container.amount);
      if (bare !== null) return bare;
      const bareCents = asNumber(container.amount_cents);
      if (bareCents !== null) return bareCents / 100;
    }
  }

  const bare = asNumber(value.balance);
  if (bare !== null) {
    const currency = stringField(value.currency)?.toUpperCase();
    if (!currency || currency === upper) return bare;
  }
  const bareCredits = asNumber(value.credits);
  if (bareCredits !== null) {
    const currency = stringField(value.currency)?.toUpperCase();
    if (!currency || currency === upper) return bareCredits;
  }
  return null;
}

function hourlyMoney(value: unknown, unit: "eur" | "usd"): number | null {
  if (!isRecord(value)) return null;
  const upper = unit.toUpperCase();

  const direct =
    asNumber(value[`hourly_${unit}`]) ??
    asNumber(value[`hourly_rate_${unit}`]) ??
    asNumber(value[`price_hourly_${unit}`]);
  if (direct !== null) return direct;

  const cents =
    asNumber(value[`hourly_${unit}_cents`]) ??
    asNumber(value[`hourly_cents_${unit}`]);
  if (cents !== null) return cents / 100;

  const hourly = value.hourly;
  if (isRecord(hourly)) {
    const amount = asNumber(hourly[unit] ?? hourly[upper]);
    if (amount !== null) return amount;
    const hourlyCents = asNumber(hourly[`${unit}_cents`]);
    if (hourlyCents !== null) return hourlyCents / 100;
    const objectAmount = amountObject(hourly.amount, unit);
    if (objectAmount !== null) return objectAmount;
  }

  const prices = value.prices;
  if (isRecord(prices) && isRecord(prices.hourly)) {
    const amount = asNumber(prices.hourly[unit] ?? prices.hourly[upper]);
    if (amount !== null) return amount;
    const hourlyCents = asNumber(prices.hourly[`${unit}_cents`]);
    if (hourlyCents !== null) return hourlyCents / 100;
  }

  return null;
}

function amountObject(value: unknown, unit: "eur" | "usd"): number | null {
  if (!isRecord(value)) return null;
  const currency = stringField(value.currency)?.toUpperCase();
  if (currency && currency !== unit.toUpperCase()) return null;
  const amount = asNumber(value.amount);
  if (amount !== null) return amount;
  const cents = asNumber(value.amount_cents);
  return cents === null ? null : cents / 100;
}

function firstRegionRate(
  rates: Partial<Record<PaygRegion, PaygRegionRate>>,
  unit: "eur" | "usd",
): number | null {
  for (const region of PAYG_REGIONS) {
    const rate = rates[region];
    if (!rate) continue;
    const amount = unit === "eur" ? rate.hourly_eur : rate.hourly_usd;
    if (amount !== null) return amount;
  }
  return null;
}

function planRate(
  plan: PaygPlan,
  region: PaygRegion,
  currency: PaygCurrency,
): HourlyRate | null {
  const regional = plan.region_rates[region];
  const direct = regional
    ? currency === "EUR"
      ? regional.hourly_eur
      : regional.hourly_usd
    : null;
  if (direct !== null) return { amount: direct, currency, fallback: false };

  const directPlan = currency === "EUR" ? plan.hourly_eur : plan.hourly_usd;
  if (directPlan !== null) return { amount: directPlan, currency, fallback: false };

  const other: PaygCurrency = currency === "EUR" ? "USD" : "EUR";
  const regionalOther = regional
    ? other === "EUR"
      ? regional.hourly_eur
      : regional.hourly_usd
    : null;
  if (regionalOther !== null) {
    return { amount: regionalOther, currency: other, fallback: true };
  }

  const otherPlan = other === "EUR" ? plan.hourly_eur : plan.hourly_usd;
  if (otherPlan !== null) return { amount: otherPlan, currency: other, fallback: true };
  return null;
}

function instanceRate(
  instance: PaygInstance,
  plans: PaygPlan[] | null,
  currency: PaygCurrency,
): HourlyRate | null {
  const direct = currency === "EUR" ? instance.hourly_eur : instance.hourly_usd;
  if (direct !== null) return { amount: direct, currency, fallback: false };

  const other: PaygCurrency = currency === "EUR" ? "USD" : "EUR";
  const otherDirect = other === "EUR" ? instance.hourly_eur : instance.hourly_usd;
  if (otherDirect !== null) return { amount: otherDirect, currency: other, fallback: true };

  const plan = plans?.find((candidate) => candidate.id === instance.plan_id) ?? null;
  return plan ? planRate(plan, instance.region, currency) : null;
}

function balanceIn(
  balance: PaygCreditBalance | null,
  currency: PaygCurrency,
): number | null {
  if (!balance) return null;
  return currency === "EUR" ? balance.balance_eur : balance.balance_usd;
}

function runtimeFromBalance(
  balance: PaygCreditBalance | null,
  rate: HourlyRate | null,
): number | null {
  if (!balance || !rate || rate.amount <= 0) return null;
  const amount = balanceIn(balance, rate.currency);
  if (amount === null) return null;
  return amount / rate.amount;
}

function formatMoney(amount: number, currency: PaygCurrency): string {
  return `${currency === "EUR" ? "€" : "$"}${amount.toFixed(2)}`;
}

function formatHourlyRate(rate: HourlyRate): string {
  const digits = rate.amount < 1 ? 4 : 2;
  return `${rate.currency === "EUR" ? "€" : "$"}${rate.amount.toFixed(digits)}/h`;
}

function formatTopUpAmount(amount: number, currency: PaygCurrency): string {
  const digits = Number.isInteger(amount) ? 0 : 2;
  return `${currency === "EUR" ? "€" : "$"}${amount.toFixed(digits)}`;
}

function formatRuntimeHours(hours: number): string {
  if (!Number.isFinite(hours) || hours < 0) return "unknown";
  if (hours < 1) return `${Math.max(1, Math.round(hours * 60))} min`;
  if (hours < 48) return `${hours.toFixed(1)} h`;
  return `${Math.round(hours / 24)} days`;
}

function elapsedSeconds(startedAt: string | null, nowMs: number): number | null {
  if (!startedAt) return null;
  const started = Date.parse(startedAt);
  if (Number.isNaN(started)) return null;
  return Math.max(0, Math.floor((nowMs - started) / 1_000));
}

function planSpecs(plan: PaygPlan): string {
  const memory = `${plan.ram_gb} GB RAM`;
  const cpu = plan.vcpu > 0 ? `${plan.vcpu} vCPU` : null;
  const disk =
    plan.disk_gb > 0
      ? `${plan.disk_gb} GB ${plan.disk_kind ?? "disk"}`
      : null;
  return [cpu, memory, disk].filter((part): part is string => part !== null).join(" · ");
}

function roundHourly(value: number): number {
  return Math.round(value * 100_000) / 100_000;
}

function normalizeRegion(value: unknown): PaygRegion | null {
  if (typeof value !== "string") return null;
  const text = value.trim().toUpperCase();
  if (!text) return null;
  if (
    text === "EU" ||
    text === "EUROPE" ||
    text.startsWith("EU-") ||
    ["FSN1", "NBG1", "HEL1"].includes(text)
  ) {
    return "EU";
  }
  if (
    text === "US" ||
    text === "USA" ||
    text === "UNITED STATES" ||
    text.startsWith("US-") ||
    ["ASH", "HIL"].includes(text)
  ) {
    return "US";
  }
  if (
    text === "SG" ||
    text === "SGP" ||
    text === "SINGAPORE" ||
    text.startsWith("SG-") ||
    text === "SIN"
  ) {
    return "SG";
  }
  return null;
}

function timestampField(value: unknown): string | null {
  if (typeof value === "number" && Number.isFinite(value)) {
    const date = new Date(value < 10_000_000_000 ? value * 1_000 : value);
    return Number.isNaN(date.getTime()) ? null : date.toISOString();
  }
  if (typeof value !== "string" || value.trim() === "") return null;
  const parsed = Date.parse(value);
  return Number.isNaN(parsed) ? null : new Date(parsed).toISOString();
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function stringField(value: unknown): string | null {
  if (typeof value !== "string") return null;
  const trimmed = value.trim();
  return trimmed === "" ? null : trimmed;
}

function asNumber(value: unknown): number | null {
  if (typeof value === "number" && Number.isFinite(value)) return Math.max(0, value);
  if (typeof value === "string" && value.trim() !== "") {
    const parsed = Number(value);
    if (Number.isFinite(parsed)) return Math.max(0, parsed);
  }
  return null;
}

function apiMessage(value: unknown): string | null {
  if (!isRecord(value)) return null;
  if (typeof value.message === "string") return safeText(value.message);
  if (isRecord(value.error) && typeof value.error.message === "string") {
    return safeText(value.error.message);
  }
  if (typeof value.error === "string") return safeText(value.error);
  return null;
}

function messageOf(value: unknown): string {
  if (
    isRecord(value) &&
    typeof value.message === "string" &&
    value.message.trim() !== ""
  ) {
    return safeText(value.message);
  }
  return "The Compute Credits service could not be reached. Check the network and retry.";
}

function safeText(value: string, max = 300): string {
  let clean = "";
  for (const character of value) {
    const code = character.codePointAt(0) ?? 0;
    clean += code < 0x20 || code === 0x7f ? " " : character;
  }
  const trimmed = clean.replace(/\s+/g, " ").trim();
  return trimmed.length > max ? `${trimmed.slice(0, max)}…` : trimmed;
}
