import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { ManagedHomesPanel } from "../../next/ManagedHomesPanel";
import {
  configureCloudRuntime,
  resetCloudAccount,
  useCloudAccount,
} from "../../cloud/store";
import {
  TEST_ACCOUNT,
  TEST_MACHINES,
  TEST_PROVISIONING,
  TestCloudClient,
  TestRelay,
} from "./fakes";

beforeEach(() => {
  resetCloudAccount();
});

afterEach(() => {
  resetCloudAccount();
  vi.restoreAllMocks();
});

function seedSignedIn(client: TestCloudClient, provisioning = TEST_PROVISIONING): void {
  configureCloudRuntime({ client, relay: new TestRelay() });
  useCloudAccount.setState({
    phase: "signed_in",
    account: TEST_ACCOUNT,
    machines: TEST_MACHINES,
    machinesStatus: "ready",
    selectedMachineId: "mch_online",
    provisioning,
    provisioningStatus: provisioning ? "ready" : "idle",
  });
}

describe("Managed Always-On Home plans", () => {
  it("renders every published tier, the Home 8 setup fee and prices from the API", async () => {
    const client = new TestCloudClient();
    configureCloudRuntime({ client, relay: new TestRelay() });
    render(<ManagedHomesPanel autoLoad />);

    const plans = await screen.findByTestId("managed-plans");
    expect(within(plans).getAllByRole("listitem")).toHaveLength(5);
    for (const tier of ["Home 8", "Home 12", "Home 24", "Home 48", "Home 64"]) {
      expect(within(plans).getByText(tier)).toBeInTheDocument();
    }
    expect(screen.getByTestId("plan-price-home-8")).toHaveTextContent("€6.60/mo");
    expect(screen.getByTestId("plan-price-home-64")).toHaveTextContent("€44.00/mo");
    expect(screen.getByTestId("plan-setup-home-8")).toHaveTextContent(
      "One-time setup fee €9.90",
    );
    expect(screen.queryByTestId("plan-setup-home-12")).not.toBeInTheDocument();
    expect(within(screen.getByTestId("plan-home-12")).getByText("No setup fee")).toBeInTheDocument();
    expect(client.planCalls).toBe(1);
    const text = plans.textContent ?? "";
    expect(text).toContain("Cloud VPS 4");
    expect(text).not.toContain("€99");
  });

  it("keeps R2 backups off by default with a clear Recommended badge, then toggles locally", async () => {
    const client = new TestCloudClient();
    configureCloudRuntime({ client, relay: new TestRelay() });
    const user = userEvent.setup();
    render(<ManagedHomesPanel autoLoad />);
    await screen.findByTestId("managed-plans");

    const toggle = screen.getByRole("switch", { name: /Off-Site Encrypted R2 Backups/i });
    expect(toggle).not.toBeChecked();
    expect(screen.getByTestId("backup-recommended")).toHaveTextContent("Recommended");
    expect(useCloudAccount.getState().backupsEnabled).toBe(false);

    await user.click(toggle);
    expect(toggle).toBeChecked();
    expect(useCloudAccount.getState().backupsEnabled).toBe(true);
  });

  it("does not turn backups on even when the API reports default_enabled true", async () => {
    const client = new TestCloudClient({
      plans: {
        plans: [],
        backups: { monthly_eur: 3.5, recommended: true, provider: "cloudflare-r2" },
      },
    });
    configureCloudRuntime({ client, relay: new TestRelay() });
    render(<ManagedHomesPanel autoLoad />);
    await screen.findByTestId("plans-empty");
    expect(useCloudAccount.getState().backupsEnabled).toBe(false);
  });

  it("shows a live provisioning status and updates as the phase changes", async () => {
    const client = new TestCloudClient();
    seedSignedIn(client);
    render(<ManagedHomesPanel autoLoad />);
    await screen.findByTestId("managed-plans");

    const status = screen.getByTestId("provisioning-status");
    expect(status).toHaveTextContent("Provisioning");
    expect(screen.getByTestId("provisioning-phase")).toHaveTextContent("Provisioning");
    const progress = screen.getByTestId("provisioning-progress");
    expect(progress).toHaveAttribute("aria-valuenow", "42");
    expect(status).toHaveTextContent("Installing StackSpirit");

    act(() => {
      useCloudAccount.setState({
        provisioning: {
          ...TEST_PROVISIONING,
          phase: "online",
          step: null,
          progress_pct: 100,
          detail: null,
        },
      });
    });
    expect(screen.getByTestId("provisioning-phase")).toHaveTextContent("Online");
    expect(status).toHaveTextContent("contabo-1 is online on relay.stackspirit.io.");
    expect(screen.getByRole("link", { name: "Open the Home" })).toHaveAttribute(
      "href",
      "https://app.stackspirit.io/machines/mch_online",
    );
  });

  it("says that provisioning needs a signed-in Home without hiding local mode", async () => {
    const client = new TestCloudClient();
    configureCloudRuntime({ client, relay: new TestRelay() });
    render(<ManagedHomesPanel autoLoad />);
    await screen.findByTestId("managed-plans");
    expect(screen.getByTestId("provisioning-status")).toHaveTextContent(
      /Sign in to StackSpirit to see live provisioning status/i,
    );
  });

  it("shows an honest plan error and never invents prices", async () => {
    const client = new TestCloudClient();
    vi.spyOn(client, "getPlans").mockRejectedValueOnce({
      code: "cloud_unreachable",
      message: "The StackSpirit API could not be reached.",
      hint: "Retry when the audience is reachable.",
      retryable: true,
      status: null,
    });
    configureCloudRuntime({ client, relay: new TestRelay() });
    render(<ManagedHomesPanel autoLoad />);
    const error = await screen.findByTestId("plans-error");
    expect(error).toHaveTextContent("The StackSpirit API could not be reached.");
    expect(screen.queryByTestId("managed-plans")).not.toBeInTheDocument();
    await waitFor(() => {
      expect(useCloudAccount.getState().plansStatus).toBe("error");
    });
    expect(useCloudAccount.getState().plans).toBeNull();
    expect(screen.queryByText(/€/)).not.toBeInTheDocument();
  });

  it("stays collapsed in Settings and makes no plan request until asked", () => {
    const client = new TestCloudClient();
    configureCloudRuntime({ client, relay: new TestRelay() });
    render(<ManagedHomesPanel />);
    expect(screen.getByTestId("managed-homes-panel")).toHaveTextContent("Get an Always-On Home");
    expect(screen.queryByTestId("managed-plans")).not.toBeInTheDocument();
    expect(client.planCalls).toBe(0);
  });
});
