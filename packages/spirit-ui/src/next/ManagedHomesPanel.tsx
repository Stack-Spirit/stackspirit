import { useEffect, useState } from "react";

import { useCloudAccount } from "../cloud/store";
import type { ManagedPlan, ProvisioningStatus } from "../cloud/types";
import { formatClock } from "../domain/format";
import {
  formatMonthly,
  isProvisioningLive,
  planSpecs,
  provisioningLabel,
  provisioningProgress,
  provisioningTone,
  setupFeeLabel,
} from "./cloudFormat";

export interface ManagedHomesPanelProps {
  /**
   * Load the public plan catalog as soon as the panel mounts. The Web app's
   * dedicated Always-On Homes page passes `true`; Settings keeps it collapsed so
   * opening local settings never makes a cloud request.
   */
  autoLoad?: boolean;
}

/**
 * Managed Always-On Home plans and live provisioning status.
 *
 * Prices are rendered only from `GET /v1/plans`; the panel never invents a tier
 * or a price when the API is unavailable. Off-site encrypted R2 backups are off
 * by default and carry a clear Recommended badge, matching
 * `docs/STRATEGY.md` §6.
 */
export function ManagedHomesPanel({ autoLoad = false }: ManagedHomesPanelProps) {
  const phase = useCloudAccount((state) => state.phase);
  const selectedMachineId = useCloudAccount((state) => state.selectedMachineId);
  const machines = useCloudAccount((state) => state.machines);
  const plans = useCloudAccount((state) => state.plans);
  const plansStatus = useCloudAccount((state) => state.plansStatus);
  const plansError = useCloudAccount((state) => state.plansError);
  const backupsEnabled = useCloudAccount((state) => state.backupsEnabled);
  const provisioning = useCloudAccount((state) => state.provisioning);
  const provisioningStatus = useCloudAccount((state) => state.provisioningStatus);
  const provisioningError = useCloudAccount((state) => state.provisioningError);
  const loadPlans = useCloudAccount((state) => state.loadPlans);
  const setBackupsEnabled = useCloudAccount((state) => state.setBackupsEnabled);
  const refreshProvisioning = useCloudAccount((state) => state.refreshProvisioning);

  const [expanded, setExpanded] = useState(autoLoad);
  const signedIn = phase === "signed_in";

  useEffect(() => {
    if (!expanded || plansStatus !== "idle") return;
    void loadPlans();
  }, [expanded, plansStatus, loadPlans]);

  useEffect(() => {
    if (!expanded || !signedIn || !selectedMachineId) return;
    if (provisioningStatus === "idle") {
      void refreshProvisioning(selectedMachineId);
      return;
    }
    if (!isProvisioningLive(provisioning)) return;
    const timer = setInterval(() => {
      void refreshProvisioning(selectedMachineId);
    }, 8_000);
    return () => clearInterval(timer);
  }, [
    expanded,
    signedIn,
    selectedMachineId,
    provisioningStatus,
    provisioning,
    refreshProvisioning,
  ]);

  const selectedMachine =
    machines.find((machine) => machine.id === selectedMachineId) ?? null;

  if (!expanded) {
    return (
      <section
        className="nx-cloud nx-cloud--plans"
        data-testid="managed-homes-panel"
        aria-label="Get an Always-On Home"
      >
        <div className="nx-cloud__title">
          <div>
            <h3>Get an Always-On Home</h3>
            <p>
              Turnkey managed machines are optional. StackSpirit itself stays free and
              keeps working with your own server.
            </p>
          </div>
          <button
            type="button"
            className="nx-btn nx-btn--quiet nx-btn--sm"
            onClick={() => setExpanded(true)}
            data-testid="managed-homes-expand"
          >
            See plans
          </button>
        </div>
      </section>
    );
  }

  return (
    <section
      className="nx-cloud nx-cloud--plans"
      data-testid="managed-homes-panel"
      aria-label="Get an Always-On Home"
    >
      <div className="nx-cloud__title">
        <div>
          <h3>Get an Always-On Home</h3>
          <p>
            Turnkey Contabo machines at Contabo&apos;s regular monthly rate plus 20%.
            The software stays free and MIT; cancel the machine anytime.
          </p>
        </div>
        <span className="nx-chip nx-chip--muted">Monthly · EUR</span>
      </div>

      {plansStatus === "loading" && !plans ? (
        <p className="nx-cloud__meta" role="status" data-testid="plans-loading">
          Loading the published plans…
        </p>
      ) : plansStatus === "error" ? (
        <div className="nx-cloud__error" role="alert" data-testid="plans-error">
          <p>
            {plansError?.message ??
              "Managed Home plans are unavailable right now. StackSpirit never guesses a price."}
            {plansError?.hint ? <span> {plansError.hint}</span> : null}
          </p>
          <button
            type="button"
            className="nx-btn nx-btn--quiet nx-btn--sm"
            onClick={() => void loadPlans()}
          >
            Retry
          </button>
        </div>
      ) : !plans || plans.plans.length === 0 ? (
        <p className="nx-cloud__empty" data-testid="plans-empty">
          No managed plans are published right now. Local and SSH servers are unaffected.
        </p>
      ) : (
        <>
          <ul className="nx-cloud__plans" data-testid="managed-plans" aria-label="Managed Home plans">
            {plans.plans.map((plan) => (
              <PlanCard key={plan.id} plan={plan} />
            ))}
          </ul>

          <label className="nx-cloud__backup" htmlFor="managed-home-backups">
            <span className="nx-cloud__backup-main">
              <input
                id="managed-home-backups"
                type="checkbox"
                role="switch"
                checked={backupsEnabled}
                onChange={(event) => setBackupsEnabled(event.currentTarget.checked)}
                data-testid="backup-toggle"
              />
              <span>
                <strong>Off-Site Encrypted R2 Backups</strong>
                {plans.backups.recommended ? (
                  <span className="nx-chip nx-chip--warn" data-testid="backup-recommended">
                    Recommended
                  </span>
                ) : null}
                <small>
                  Off by default. Encrypts ~/.stackspirit data, sessions and configs to
                  Cloudflare R2, off the machine provider.{" "}
                  {plans.backups.monthly_eur === null
                    ? "Priced separately at checkout."
                    : `${formatMonthly(plans.backups.monthly_eur)}.`}
                </small>
              </span>
            </span>
          </label>
        </>
      )}

      <div className="nx-cloud__provisioning" data-testid="provisioning-status">
        <div className="nx-cloud__provisioning-head">
          <h4>Provisioning</h4>
          {provisioning ? (
            <span
              className={`nx-chip nx-chip--${
                provisioningTone(provisioning.phase) === "ok"
                  ? "server"
                  : provisioningTone(provisioning.phase)
              }`}
              data-testid="provisioning-phase"
            >
              {provisioningLabel(provisioning.phase)}
            </span>
          ) : null}
        </div>
        {!signedIn ? (
          <p className="nx-cloud__meta">
            Sign in to StackSpirit to see live provisioning status. Local mode stays
            available without an account.
          </p>
        ) : !selectedMachine ? (
          <p className="nx-cloud__meta">
            Select an Always-On Home above to see its provisioning status.
          </p>
        ) : provisioningStatus === "loading" && !provisioning ? (
          <p className="nx-cloud__meta" role="status">
            Checking provisioning status…
          </p>
        ) : provisioningStatus === "error" ? (
          <p className="nx-cloud__error" role="alert" data-testid="provisioning-error">
            {provisioningError?.message ?? "Provisioning status could not be read."}
          </p>
        ) : provisioning ? (
          <ProvisioningBody provisioning={provisioning} machineName={selectedMachine.name} />
        ) : (
          <p className="nx-cloud__meta" data-testid="provisioning-none">
            No provisioning record for {selectedMachine.name} yet. A managed Home shows
            its cloud-init progress here once checkout starts it.
          </p>
        )}
      </div>
    </section>
  );
}

function PlanCard({ plan }: { plan: ManagedPlan }) {
  const setup = setupFeeLabel(plan);
  return (
    <li className="nx-cloud__plan" data-testid={`plan-${plan.id}`}>
      <div className="nx-cloud__plan-head">
        <strong>{plan.name}</strong>
        <span className="nx-cloud__price" data-testid={`plan-price-${plan.id}`}>
          {formatMonthly(plan.monthly_eur)}
        </span>
      </div>
      <span className="nx-cloud__meta">{planSpecs(plan)}</span>
      {plan.provider_spec ? (
        <span className="nx-cloud__meta">{plan.provider_spec}</span>
      ) : null}
      {setup ? (
        <span className="nx-cloud__setup" data-testid={`plan-setup-${plan.id}`}>
          {setup}
        </span>
      ) : (
        <span className="nx-cloud__setup nx-cloud__setup--none">No setup fee</span>
      )}
    </li>
  );
}

function ProvisioningBody({
  provisioning,
  machineName,
}: {
  provisioning: ProvisioningStatus;
  machineName: string;
}) {
  const progress = provisioningProgress(provisioning);
  const homeUrl = safeHomeUrl(provisioning.home_url);
  return (
    <div className="nx-cloud__provisioning-body">
      <p className="nx-cloud__meta">
        {provisioning.step ?? `Provisioning ${machineName}`}
        {provisioning.detail ? <span> {provisioning.detail}</span> : null}
      </p>
      {progress !== null ? (
        <div
          className="nx-cloud__progress"
          role="progressbar"
          aria-label={`Provisioning ${machineName}`}
          aria-valuemin={0}
          aria-valuemax={100}
          aria-valuenow={progress}
          data-testid="provisioning-progress"
        >
          <span className="nx-cloud__progress-fill" style={{ width: `${progress}%` }} />
        </div>
      ) : null}
      <p className="nx-cloud__meta">
        Updated {formatClock(provisioning.updated_at)}
        {homeUrl ? (
          <>
            {" · "}
            <a className="nx-link" href={homeUrl} target="_blank" rel="noreferrer noopener">
              Open the Home
            </a>
          </>
        ) : null}
      </p>
      {provisioning.phase === "online" ? (
        <p className="nx-cloud__relay nx-cloud__relay--ok" role="status">
          {machineName} is online on relay.stackspirit.io.
        </p>
      ) : null}
    </div>
  );
}

function safeHomeUrl(value: string | null): string | null {
  if (!value) return null;
  try {
    const url = new URL(value);
    if (url.protocol === "https:" || url.protocol === "http:") return url.toString();
  } catch {
    return null;
  }
  return null;
}
