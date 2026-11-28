import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  configureCloudRuntime,
  resetCloudAccount,
  useCloudAccount,
} from "../../cloud/store";
import type { CloudFetch } from "../../cloud/types";
import {
  createPaygHttpApi,
  FALLBACK_PAYG_PLANS,
  PaygComputePanel,
  type PaygComputeApi,
  type PaygCreditBalance,
  type PaygCurrency,
  type PaygInstance,
  type PaygPlan,
  type PaygTopUpResult,
} from "../../next/PaygComputePanel";
import { ManagedHomesPanel } from "../../next/ManagedHomesPanel";
import { TEST_ACCOUNT, TestCloudClient, TestRelay } from "../cloud/fakes";

beforeEach(() => {
  resetCloudAccount();
});

afterEach(() => {
  resetCloudAccount();
  vi.useRealTimers();
});

const NOW = Date.parse("2026-11-28T12:00:00.000Z");

const TEST_PAYG_PLANS: PaygPlan[] = [
  {
    id: "hetzner-4gb",
    name: "Hetzner 4 GB",
    vcpu: 2,
    ram_gb: 4,
    disk_gb: 40,
    disk_kind: "SSD",
    provider_spec: "Hetzner Cloud",
    hourly_eur: 0.0144,
    hourly_usd: 0.0157,
    regions: ["EU", "US", "SG"],
    region_rates: {
      US: { hourly_eur: 0.02, hourly_usd: 0.0218 },
    },
  },
  {
    id: "hetzner-8gb",
    name: "Hetzner 8 GB",
    vcpu: 4,
    ram_gb: 8,
    disk_gb: 80,
    disk_kind: "SSD",
    provider_spec: "Hetzner Cloud",
    hourly_eur: 0.0288,
    hourly_usd: 0.0314,
    regions: ["EU", "US", "SG"],
    region_rates: {},
  },
  {
    id: "hetzner-16gb",
    name: "Hetzner 16 GB",
    vcpu: 8,
    ram_gb: 16,
    disk_gb: 160,
    disk_kind: "SSD",
    provider_spec: "Hetzner Cloud",
    hourly_eur: 0.0576,
    hourly_usd: 0.0628,
    regions: ["EU", "US", "SG"],
    region_rates: {},
  },
  {
    id: "hetzner-32gb",
    name: "Hetzner 32 GB",
    vcpu: 16,
    ram_gb: 32,
    disk_gb: 320,
    disk_kind: "SSD",
    provider_spec: "Hetzner Cloud",
    hourly_eur: 0.1152,
    hourly_usd: 0.1256,
    regions: ["EU", "US", "SG"],
    region_rates: {},
  },
  {
    id: "hetzner-64gb",
    name: "Hetzner 64 GB",
    vcpu: 32,
    ram_gb: 64,
    disk_gb: 640,
    disk_kind: "SSD",
    provider_spec: "Hetzner Cloud",
    hourly_eur: 0.2304,
    hourly_usd: null,
    regions: ["EU", "US", "SG"],
    region_rates: {},
  },
];

const TEST_BALANCE: PaygCreditBalance = {
  balance_eur: 12.5,
  balance_usd: 13.75,
  updated_at: "2026-11-28T11:55:00.000Z",
};

const TEST_INSTANCE: PaygInstance = {
  id: "inst_1",
  name: "hetzner-builder",
  plan_id: "hetzner-8gb",
  plan_name: "Hetzner 8 GB",
  region: "EU",
  started_at: "2026-11-28T10:30:00.000Z",
  hourly_eur: 0.0288,
  hourly_usd: 0.0314,
};

interface FakePaygOptions {
  plans?: PaygPlan[];
  plansError?: Error;
  balance?: PaygCreditBalance;
  balanceError?: Error;
  instances?: PaygInstance[];
  instancesError?: Error;
  checkoutUrl?: string;
  stopError?: Error;
}

class FakePaygApi implements PaygComputeApi {
  planCalls = 0;
  balanceCalls = 0;
  instanceCalls = 0;
  topUps: Array<{ amount: number; currency: PaygCurrency }> = [];
  stopped: string[] = [];
  private readonly options: FakePaygOptions;

  constructor(options: FakePaygOptions = {}) {
    this.options = options;
  }

  async getPaygPlans(): Promise<PaygPlan[]> {
    this.planCalls += 1;
    if (this.options.plansError) throw this.options.plansError;
    return this.options.plans ?? TEST_PAYG_PLANS;
  }

  async getCreditBalance(): Promise<PaygCreditBalance> {
    this.balanceCalls += 1;
    if (this.options.balanceError) throw this.options.balanceError;
    return this.options.balance ?? TEST_BALANCE;
  }

  async getPaygInstances(): Promise<PaygInstance[]> {
    this.instanceCalls += 1;
    if (this.options.instancesError) throw this.options.instancesError;
    return this.options.instances ?? [];
  }

  async topUpCredits(amount: number, currency: PaygCurrency): Promise<PaygTopUpResult> {
    this.topUps.push({ amount, currency });
    return {
      checkout_url:
        this.options.checkoutUrl ?? "https://checkout.stripe.com/c/pay/test_session",
    };
  }

  async stopPaygInstance(instanceId: string): Promise<void> {
    if (this.options.stopError) throw this.options.stopError;
    this.stopped.push(instanceId);
  }
}

function seedSignedIn(): void {
  configureCloudRuntime({ client: new TestCloudClient(), relay: new TestRelay() });
  useCloudAccount.setState({
    phase: "signed_in",
    account: TEST_ACCOUNT,
    machinesStatus: "ready",
  });
}

function json(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "Content-Type": "application/json" },
  });
}

describe("Pay-As-You-Go Compute plan tab", () => {
  it("keeps the monthly catalog by default and switches cleanly to hourly compute", async () => {
    seedSignedIn();
    const payg = new FakePaygApi();
    const user = userEvent.setup();
    render(<ManagedHomesPanel autoLoad paygApi={payg} />);

    await screen.findByTestId("managed-plans");
    expect(screen.getByTestId("managed-homes-tab-monthly")).toHaveAttribute(
      "aria-selected",
      "true",
    );
    expect(screen.getByTestId("provisioning-status")).toBeInTheDocument();

    await user.click(screen.getByRole("tab", { name: "Pay-As-You-Go (Hourly)" }));

    expect(screen.getByTestId("managed-homes-tab-payg")).toHaveAttribute(
      "aria-selected",
      "true",
    );
    expect(screen.queryByTestId("managed-plans")).not.toBeInTheDocument();
    expect(screen.queryByTestId("provisioning-status")).not.toBeInTheDocument();
    expect(await screen.findByTestId("payg-balance")).toHaveTextContent("€12.50");
    expect(payg.planCalls).toBe(1);

    await user.click(screen.getByRole("tab", { name: "Always-On Monthly" }));
    expect(await screen.findByTestId("managed-plans")).toBeInTheDocument();
    expect(screen.queryByTestId("payg-compute-panel")).not.toBeInTheDocument();
  });
});

describe("Pay-As-You-Go Compute panel", () => {
  it("lists Hetzner 4 GB through 64 GB with hourly +20% rates, specs and runtime", async () => {
    const api = new FakePaygApi();
    const user = userEvent.setup();
    render(<PaygComputePanel api={api} signedIn now={() => NOW} />);

    const plans = await screen.findByTestId("payg-plans");
    expect(within(plans).getAllByRole("listitem")).toHaveLength(5);
    for (const tier of [
      "Hetzner 4 GB",
      "Hetzner 8 GB",
      "Hetzner 16 GB",
      "Hetzner 32 GB",
      "Hetzner 64 GB",
    ]) {
      expect(within(plans).getByText(tier)).toBeInTheDocument();
    }
    expect(screen.getByText("+20% included")).toBeInTheDocument();
    expect(screen.getByTestId("payg-rate-hetzner-4gb")).toHaveTextContent("€0.0144/h");
    expect(screen.getByTestId("payg-specs-hetzner-4gb")).toHaveTextContent(
      "2 vCPU · 4 GB RAM · 40 GB SSD",
    );
    expect(screen.getByTestId("payg-runtime-hetzner-4gb")).toHaveTextContent(
      "≈ 36 days from your balance",
    );

    await user.click(screen.getByTestId("payg-region-us"));
    expect(screen.getByTestId("payg-rate-hetzner-4gb")).toHaveTextContent("€0.0200/h");
    expect(screen.getByTestId("payg-runtime-hetzner-4gb")).toHaveTextContent(
      "≈ 26 days from your balance",
    );
  });

  it("tops up from a quick preset or a valid custom amount and redirects to Stripe", async () => {
    const api = new FakePaygApi();
    const navigate = vi.fn();
    const user = userEvent.setup();
    render(
      <PaygComputePanel api={api} signedIn navigate={navigate} now={() => NOW} />,
    );
    await screen.findByTestId("payg-balance");

    await user.click(screen.getByTestId("payg-topup-preset-25"));
    await waitFor(() => {
      expect(api.topUps).toEqual([{ amount: 25, currency: "EUR" }]);
    });
    expect(navigate).toHaveBeenCalledWith(
      "https://checkout.stripe.com/c/pay/test_session",
    );
    expect(screen.getByTestId("payg-topup-notice")).toHaveTextContent(
      "Opening Stripe Checkout for €25",
    );

    const input = screen.getByTestId("payg-topup-custom");
    await user.clear(input);
    await user.type(input, "4");
    await user.click(screen.getByTestId("payg-topup-submit"));
    expect(screen.getByTestId("payg-topup-error")).toHaveTextContent(/at least €5/);
    expect(api.topUps).toHaveLength(1);

    await user.clear(input);
    await user.type(input, "12.5");
    await user.click(screen.getByTestId("payg-topup-submit"));
    await waitFor(() => {
      expect(api.topUps).toEqual([
        { amount: 25, currency: "EUR" },
        { amount: 12.5, currency: "EUR" },
      ]);
    });
  });

  it("refuses an unsafe checkout URL and never navigates there", async () => {
    const api = new FakePaygApi({ checkoutUrl: "javascript:alert(1)" });
    const navigate = vi.fn();
    const user = userEvent.setup();
    render(
      <PaygComputePanel api={api} signedIn navigate={navigate} now={() => NOW} />,
    );
    await screen.findByTestId("payg-balance");

    await user.click(screen.getByTestId("payg-topup-preset-10"));
    expect(await screen.findByTestId("payg-topup-error")).toHaveTextContent(
      /safe Stripe Checkout link/i,
    );
    expect(navigate).not.toHaveBeenCalled();
  });

  it("switches the balance, presets and rates between EUR and USD without guessing", async () => {
    const api = new FakePaygApi();
    const user = userEvent.setup();
    render(<PaygComputePanel api={api} signedIn now={() => NOW} />);
    await screen.findByTestId("payg-balance");
    expect(screen.getByTestId("payg-balance")).toHaveTextContent("€12.50");
    expect(screen.getByTestId("payg-topup-preset-10")).toHaveTextContent("€10");
    expect(screen.getByTestId("payg-rate-hetzner-4gb")).toHaveTextContent("€0.0144/h");

    await user.click(screen.getByTestId("payg-currency-usd"));
    expect(screen.getByTestId("payg-balance")).toHaveTextContent("$13.75");
    expect(screen.getByTestId("payg-topup-preset-10")).toHaveTextContent("$10");
    expect(screen.getByTestId("payg-rate-hetzner-4gb")).toHaveTextContent("$0.0157/h");
    expect(screen.getByTestId("payg-rate-fallback")).toHaveTextContent(
      /USD hourly rates are not published/i,
    );
    expect(screen.getByTestId("payg-rate-hetzner-64gb")).toHaveTextContent("€0.2304/h");
  });

  it("shows reference fallback tiers when GET /v1/payg/plans is unavailable", async () => {
    const api = new FakePaygApi({
      plansError: new Error("Live Pay-As-You-Go rates are unavailable."),
    });
    render(<PaygComputePanel api={api} signedIn now={() => NOW} />);

    expect(await screen.findByTestId("payg-plans-fallback")).toHaveTextContent(
      /reference Hetzner tiers/i,
    );
    const plans = screen.getByTestId("payg-plans");
    expect(within(plans).getAllByRole("listitem")).toHaveLength(5);
    expect(within(plans).getByText("Hetzner 4 GB")).toBeInTheDocument();
    expect(within(plans).getByText("Hetzner 64 GB")).toBeInTheDocument();
    expect(screen.getByTestId("payg-rate-hetzner-4gb")).toHaveTextContent("€0.0073/h");
    expect(FALLBACK_PAYG_PLANS).toHaveLength(5);
  });

  it("shows live elapsed time and burn rate, then stops billing on confirmation", async () => {
    const api = new FakePaygApi({ instances: [TEST_INSTANCE] });
    const user = userEvent.setup();
    render(<PaygComputePanel api={api} signedIn now={() => NOW} />);

    await screen.findByTestId("payg-instance-inst_1");
    expect(screen.getByTestId("payg-elapsed-inst_1")).toHaveTextContent(
      "Running 1h 30m",
    );
    expect(screen.getByTestId("payg-burn-inst_1")).toHaveTextContent("€0.0288/h");

    await user.click(screen.getByTestId("payg-stop-inst_1"));
    const confirm = screen.getByTestId("payg-stop-confirm-inst_1");
    expect(confirm).toHaveTextContent("Billing stops immediately.");
    expect(confirm).toHaveTextContent(/no further hourly charges/i);
    expect(api.stopped).toEqual([]);

    await user.click(screen.getByTestId("payg-confirm-stop-inst_1"));
    await waitFor(() => {
      expect(api.stopped).toEqual(["inst_1"]);
    });
    expect(screen.queryByTestId("payg-instance-inst_1")).not.toBeInTheDocument();
    expect(screen.getByTestId("payg-stop-notice")).toHaveTextContent(
      "Billing stopped immediately.",
    );
    expect(screen.getByTestId("payg-instances-empty")).toBeInTheDocument();
  });

  it("keeps the elapsed clock moving for a running machine", async () => {
    let clock = Date.parse("2026-11-28T10:00:30.000Z");
    const api = new FakePaygApi({
      instances: [
        {
          ...TEST_INSTANCE,
          started_at: "2026-11-28T10:00:00.000Z",
        },
      ],
    });
    render(<PaygComputePanel api={api} signedIn now={() => clock} />);

    const elapsed = await screen.findByTestId("payg-elapsed-inst_1");
    expect(elapsed).toHaveTextContent("Running 30s");

    clock += 1_000;
    await waitFor(
      () => {
        expect(elapsed).toHaveTextContent("Running 31s");
      },
      { timeout: 2_500 },
    );
  });

  it("explains the zero-surprise guardrail and the signed-out state", async () => {
    const api = new FakePaygApi();
    render(<PaygComputePanel api={api} now={() => NOW} />);

    expect(await screen.findByTestId("payg-sign-in")).toHaveTextContent(
      /Sign in to StackSpirit/i,
    );
    expect(api.balanceCalls).toBe(0);
    expect(api.instanceCalls).toBe(0);
    expect(screen.getByTestId("payg-guardrail")).toHaveTextContent(
      /less than 2 hours/i,
    );
    expect(screen.getByTestId("payg-guardrail")).toHaveTextContent(
      /shuts down automatically/i,
    );
    expect(screen.getByTestId("payg-guardrail")).toHaveTextContent(/€0/);
  });
});

describe("Pay-As-You-Go HTTP transport", () => {
  it("calls the documented endpoints with the bearer session only where needed", async () => {
    const calls: Array<{
      method: string;
      path: string;
      headers: Headers;
      body: unknown;
    }> = [];
    const fetchImpl: CloudFetch = async (input, init = {}) => {
      const raw =
        typeof input === "string"
          ? input
          : input instanceof URL
            ? input.toString()
            : input.url;
      const url = new URL(raw);
      const method = init.method ?? "GET";
      calls.push({
        method,
        path: url.pathname,
        headers: new Headers(init.headers),
        body: typeof init.body === "string" ? (JSON.parse(init.body) as unknown) : null,
      });
      const key = `${method} ${url.pathname}`;
      switch (key) {
        case "GET /v1/payg/plans":
          return json({ plans: TEST_PAYG_PLANS.slice(0, 1) });
        case "GET /v1/credits/balance":
          return json({
            balance: { eur: 12.5, usd: 13.75 },
            updated_at: "2026-11-28T11:55:00.000Z",
          });
        case "GET /v1/payg/instances":
          return json({ instances: [TEST_INSTANCE] });
        case "POST /v1/credits/topup":
          return json({ checkout_url: "https://checkout.stripe.com/c/pay/test" });
        case "DELETE /v1/payg/instances/inst_1":
          return new Response(null, { status: 204 });
        default:
          return json({ message: "not found" }, 404);
      }
    };

    const api = createPaygHttpApi({
      apiBaseUrl: "https://api.test/",
      fetchImpl,
      getAccessToken: () => "session-token",
    });

    const plans = await api.getPaygPlans();
    const balance = await api.getCreditBalance();
    const instances = await api.getPaygInstances();
    const topUp = await api.topUpCredits(10, "USD");
    await api.stopPaygInstance("inst_1");

    expect(plans).toHaveLength(1);
    expect(plans[0]?.hourly_eur).toBe(0.0144);
    expect(balance.balance_eur).toBe(12.5);
    expect(balance.balance_usd).toBe(13.75);
    expect(instances[0]?.id).toBe("inst_1");
    expect(topUp.checkout_url).toBe("https://checkout.stripe.com/c/pay/test");

    expect(calls.map((call) => `${call.method} ${call.path}`)).toEqual([
      "GET /v1/payg/plans",
      "GET /v1/credits/balance",
      "GET /v1/payg/instances",
      "POST /v1/credits/topup",
      "DELETE /v1/payg/instances/inst_1",
    ]);
    expect(calls[0]?.headers.get("authorization")).toBeNull();
    expect(calls[1]?.headers.get("authorization")).toBe("Bearer session-token");
    expect(calls[3]?.headers.get("authorization")).toBe("Bearer session-token");
    expect(calls[4]?.headers.get("authorization")).toBe("Bearer session-token");
    expect(calls[3]?.body).toEqual({ amount: 10, currency: "USD" });
  });
});
