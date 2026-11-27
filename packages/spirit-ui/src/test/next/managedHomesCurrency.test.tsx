import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import type { ManagedPlanCatalog } from "../../cloud/types";
import {
  configureCloudRuntime,
  resetCloudAccount,
  useCloudAccount,
} from "../../cloud/store";
import { ManagedHomesPanel } from "../../next/ManagedHomesPanel";
import { TestCloudClient, TestRelay } from "../cloud/fakes";

beforeEach(() => {
  resetCloudAccount();
});

afterEach(() => {
  resetCloudAccount();
});

const USD_PLANS = {
  plans: [
    {
      id: "home-8",
      name: "Home 8",
      vcpu: 4,
      ram_gb: 8,
      disk_gb: 100,
      disk_kind: "SSD",
      monthly_eur: 6.6,
      monthly_usd: 7.2,
      setup_fee_eur: 9.9,
      setup_fee_usd: 10.8,
      currency: "EUR",
      provider_spec: "Cloud VPS 4",
    },
  ],
  backups: {
    monthly_eur: 3.5,
    monthly_usd: 3.8,
    recommended: true,
    provider: "cloudflare-r2",
  },
} as unknown as ManagedPlanCatalog;

describe("Managed Homes EUR / USD toggle", () => {
  it("starts in EUR and switches the tier, setup fee and backup to published USD prices", async () => {
    const client = new TestCloudClient({ plans: USD_PLANS });
    configureCloudRuntime({ client, relay: new TestRelay() });
    const user = userEvent.setup();
    render(<ManagedHomesPanel autoLoad />);

    const plans = await screen.findByTestId("managed-plans");
    expect(screen.getByTestId("plan-price-home-8")).toHaveTextContent("€6.60/mo");
    expect(screen.getByTestId("plan-setup-home-8")).toHaveTextContent("€9.90");

    await user.click(screen.getByTestId("currency-usd"));
    expect(screen.getByTestId("plan-price-home-8")).toHaveTextContent("$7.20/mo");
    expect(screen.getByTestId("plan-setup-home-8")).toHaveTextContent("$10.80");
    expect(within(plans).getByText(/Cloud VPS 4/)).toBeInTheDocument();
    expect(
      screen.getByRole("switch", { name: /Off-Site Encrypted R2 Backups/i })
        .closest("label"),
    ).toHaveTextContent("$3.80/mo");
  });

  it("falls back to EUR with an explicit note when the API publishes no USD fields", async () => {
    const client = new TestCloudClient();
    configureCloudRuntime({ client, relay: new TestRelay() });
    const user = userEvent.setup();
    render(<ManagedHomesPanel autoLoad />);
    await screen.findByTestId("managed-plans");

    await user.click(screen.getByTestId("currency-usd"));
    expect(screen.getByTestId("currency-fallback")).toHaveTextContent(
      /USD prices are not published/i,
    );
    expect(screen.getByTestId("plan-price-home-8")).toHaveTextContent("€6.60/mo");
    expect(useCloudAccount.getState().plans?.plans[0]?.monthly_eur).toBe(6.6);
  });
});
