import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import axe from "axe-core";
import { beforeEach, describe, expect, it } from "vitest";

import {
  resetCloudAccount,
  useCloudAccount,
  type MockScenario,
  type SpiritBridge,
} from "@stackspirit/ui";

import { WebMockBridge } from "../bridge/WebMockBridge";
import {
  CAPACITY_DETAIL_EXPLAIN,
  CAPACITY_DETAIL_UNAVAILABLE,
  ENGINE_DESKTOP_ONLY,
  GATE_FAILURES_UNAVAILABLE,
  HOSTS_DESKTOP_ONLY,
  JOB_DETAIL_NOT_FOUND,
  JOB_DETAIL_UNAVAILABLE,
} from "../copy";
import { WebApp } from "./WebApp";

const WCAG_TAGS = ["wcag2a", "wcag2aa", "wcag21aa", "wcag22aa"];

async function renderApp(scenario: MockScenario = "fresh") {
  const bridge = new WebMockBridge(scenario);
  const result = render(<WebApp bridge={bridge} />);
  await screen.findByTestId("web-lanes");
  return { bridge, ...result };
}

beforeEach(() => {
  resetCloudAccount();
  window.history.replaceState(null, "", "/");
});

describe("Web read-only shell", () => {
  it("renders the preview board, lanes, running cards, and the persistent badge", async () => {
    await renderApp();
    expect(screen.getByRole("heading", { name: "Fleet", level: 1 })).toBeInTheDocument();
    expect(screen.getAllByText("Preview data").length).toBeGreaterThan(0);
    expect(screen.getByTestId("web-lanes")).toBeInTheDocument();
    expect(screen.getByTestId("running-workers")).toBeInTheDocument();
    expect(screen.getByTestId("web-structure")).toBeInTheDocument();
    expect(screen.getByTestId("lane-count-ready")).toHaveTextContent("3");
    expect(screen.getByTestId("lane-review_fix")).toBeInTheDocument();
    expect(screen.getByTestId("daemon-panel")).toBeInTheDocument();
    expect(screen.getAllByText("read-only").length).toBeGreaterThan(0);
    expect(screen.getByTestId("nav-cloud")).toHaveTextContent("Always-On Homes");
  });

  it("shows the exact capacity unavailable copy and no zeroes, target, or retrying", async () => {
    await renderApp();
    const capacity = screen.getByTestId("capacity-panel");
    expect(within(capacity).getByTestId("capacity-unavailable")).toHaveTextContent(
      CAPACITY_DETAIL_UNAVAILABLE,
    );
    expect(within(capacity).getByText(CAPACITY_DETAIL_EXPLAIN)).toBeInTheDocument();
    const text = capacity.textContent ?? "";
    expect(text).not.toMatch(/retrying/i);
    expect(text).not.toMatch(/\b0 B\b/);
    expect(text).not.toMatch(/\b0%/);
    expect(text).not.toMatch(/target 0/i);
    expect(text).not.toMatch(/advised/i);
  });

  it("shows real review/fix gate failures instead of the old unavailable placeholder", async () => {
    await renderApp();
    const lane = screen.getByTestId("lane-review_fix");
    expect(within(lane).getByTestId("lane-count-review_fix")).toHaveTextContent("1");
    const card = within(lane).getByTestId("job-card-ss-9-gate-check");
    expect(card).toHaveTextContent("secret_scan");
    expect(within(lane).queryByTestId("gate-failures-unavailable")).not.toBeInTheDocument();
    expect(lane.textContent).not.toContain(GATE_FAILURES_UNAVAILABLE);
    expect(lane.textContent).not.toMatch(/Gate failure detail is unavailable/);
  });

  it("opens a job drawer with the real job, runs, and gate failures", async () => {
    const user = userEvent.setup();
    await renderApp();
    await user.click(screen.getByTestId("job-card-ss-9-gate-check"));

    const fields = await screen.findByTestId("job-detail-fields");
    expect(screen.getByTestId("web-job-detail")).toHaveTextContent("Gate check");
    expect(fields).toHaveTextContent("reviewing");
    expect(fields).toHaveTextContent("Verifier reported a failing report gate.");
    expect(fields).toHaveTextContent(".vowp/jobs/ss-9-gate-check/report.md");

    const runs = screen.getByTestId("job-runs");
    expect(runs).toHaveTextContent("Runs (2)");
    expect(screen.getByTestId("job-run-01MOCKRUN00000000000000001")).toHaveTextContent(
      "Implemented the gate check",
    );
    expect(screen.getByTestId("job-run-01MOCKRUN00000000000000002")).toHaveTextContent(
      "exit 1",
    );

    const gates = screen.getByTestId("job-gates");
    expect(gates).toHaveTextContent("Gates — newest attempt");
    expect(screen.getByTestId("job-gate-0")).toHaveTextContent("report");
    expect(screen.getByTestId("job-gate-0")).toHaveTextContent("failed");
    expect(screen.getByTestId("job-gate-0")).toHaveTextContent("missing: tests");
    expect(screen.getByTestId("job-gate-2")).toHaveTextContent("secret_scan");

    expect(screen.queryByTestId("gate-failures-unavailable")).not.toBeInTheDocument();
    expect(screen.queryByText(GATE_FAILURES_UNAVAILABLE)).not.toBeInTheDocument();
    expect(screen.queryByText(JOB_DETAIL_UNAVAILABLE)).not.toBeInTheDocument();
    const text = document.body.textContent ?? "";
    expect(text).not.toContain("/home/");
    expect(text).not.toContain("VOWP_API_TOKEN");
  });

  it("shows a true no-runs and no-gates state for a job that never ran", async () => {
    const user = userEvent.setup();
    await renderApp();
    await user.click(screen.getByTestId("job-card-ss-5-mock"));
    await screen.findByTestId("job-detail-fields");
    expect(screen.getByTestId("job-runs")).toHaveTextContent("No runs recorded");
    expect(screen.getByTestId("job-gates")).toHaveTextContent("No gate results");
    expect(screen.queryByTestId("gate-failures-unavailable")).not.toBeInTheDocument();
  });

  it("shows a typed transport error in the drawer and never invents detail", async () => {
    const user = userEvent.setup();
    await renderApp("job-detail-error");
    await user.click(screen.getByTestId("job-card-ss-9-gate-check"));
    const error = await screen.findByTestId("job-detail-error");
    expect(error).toHaveTextContent("The daemon did not answer");
    expect(error).toHaveTextContent("mock.invalid");
    expect(screen.queryByTestId("job-detail-fields")).not.toBeInTheDocument();
    expect(screen.queryByTestId("job-runs")).not.toBeInTheDocument();
    expect(screen.queryByTestId("job-gates")).not.toBeInTheDocument();
  });

  it("shows an explicit loading state while the job detail read is pending", async () => {
    const user = userEvent.setup();
    const base = new WebMockBridge("fresh");
    const pending = new Promise<never>(() => undefined);
    const bridge = {
      ...base,
      fleet: { ...base.fleet, job: () => pending },
    } as SpiritBridge;
    render(<WebApp bridge={bridge} />);
    await screen.findByTestId("web-lanes");
    await user.click(screen.getByTestId("job-card-ss-9-gate-check"));
    expect(screen.getByTestId("job-detail-loading")).toBeInTheDocument();
    expect(screen.queryByTestId("job-detail-fields")).not.toBeInTheDocument();
  });

  it("shows the typed not-found copy for a 404 without invented detail", async () => {
    const user = userEvent.setup();
    const base = new WebMockBridge("fresh");
    const bridge = {
      ...base,
      fleet: {
        ...base.fleet,
        job: async () => {
          throw {
            code: "not_found",
            message: JOB_DETAIL_NOT_FOUND,
            hint: null,
            retryable: false,
          };
        },
      },
    } as SpiritBridge;
    render(<WebApp bridge={bridge} />);
    await screen.findByTestId("web-lanes");
    await user.click(screen.getByTestId("job-card-ss-9-gate-check"));
    const error = await screen.findByTestId("job-detail-error");
    expect(error).toHaveTextContent("Job not found");
    expect(screen.queryByTestId("job-detail-fields")).not.toBeInTheDocument();
  });

  it("shows the stale marker in the drawer and still reads the job detail", async () => {
    const user = userEvent.setup();
    await renderApp("stale-cache");
    await user.click(screen.getByTestId("job-card-ss-9-gate-check"));
    await screen.findByTestId("job-detail-fields");
    expect(screen.getByTestId("job-detail-stale")).toHaveTextContent(
      "The board snapshot is stale.",
    );
    expect(screen.getByTestId("job-gates")).toHaveTextContent("report");
  });

  it("maps an unknown preview job to not_found", async () => {
    const { bridge } = await renderApp();
    const read = bridge.fleet.job;
    if (!read) throw new Error("fleet.job missing");
    await expect(read("missing-job")).rejects.toMatchObject({ code: "not_found" });
  });

  it("keeps Hosts and Engine out of the navigation and shows exact desktop-only copy on direct URLs", async () => {
    await renderApp();
    const nav = screen.getByRole("navigation", { name: "Primary views" });
    const buttons = within(nav).getAllByRole("button");
    expect(buttons).toHaveLength(2);
    expect(buttons[0]).toHaveTextContent("Fleet");
    expect(buttons[1]).toHaveTextContent("Always-On Homes");

    window.history.pushState(null, "", "/hosts");
    window.dispatchEvent(new PopStateEvent("popstate"));
    const hosts = await screen.findByTestId("desktop-only-hosts");
    expect(hosts).toHaveTextContent(HOSTS_DESKTOP_ONLY);

    window.history.pushState(null, "", "/engine");
    window.dispatchEvent(new PopStateEvent("popstate"));
    const engine = await screen.findByTestId("desktop-only-engine");
    expect(engine).toHaveTextContent(ENGINE_DESKTOP_ONLY);
  });

  it("opens a read-only inbox with the full body and options and no answer control", async () => {
    const user = userEvent.setup();
    await renderApp();
    await user.click(screen.getByTestId("web-inbox-button"));
    const drawer = await screen.findByTestId("web-inbox-drawer");
    expect(within(drawer).getByText("Choose a merge queue policy")).toBeInTheDocument();
    expect(within(drawer).getByTestId("inbox-body")).toHaveTextContent(
      "Should the supervisor merge one job at a time or batch ready jobs?",
    );
    expect(within(drawer).getByText("One at a time")).toBeInTheDocument();
    expect(within(drawer).queryByText(/Send answer/i)).not.toBeInTheDocument();
    expect(within(drawer).queryByRole("textbox")).not.toBeInTheDocument();
  });

  it("shows the stale banner from the preview stale scenario", async () => {
    await renderApp("stale-cache");
    expect(screen.getByTestId("stale-banner")).toBeInTheDocument();
    expect(screen.getByTestId("stale-banner").textContent).toMatch(
      /no data from the daemon/i,
    );
  });

  it("shows the persistent Account menu on the cockpit even with zero machines", async () => {
    useCloudAccount.setState({
      phase: "signed_in",
      account: { id: "acct_1", email: "owner@example.com", name: "Owner" },
      machines: [],
      machinesStatus: "ready",
      machinesError: null,
      selectedMachineId: null,
      provisioning: null,
    });
    const user = userEvent.setup();
    await renderApp();

    const button = screen.getByTestId("web-account-button");
    expect(button).toHaveTextContent("Account");
    expect(button).toHaveTextContent("owner@example.com");
    expect(button).toHaveAttribute("aria-haspopup", "menu");

    await user.click(button);
    const menu = screen.getByTestId("web-account-menu");
    expect(menu).toHaveTextContent("Account details");
    expect(menu).toHaveTextContent("Managed Homes");
    expect(menu).toHaveTextContent("Sign out");

    const menuAxe = await axe.run(document.body, {
      runOnly: { type: "tag", values: WCAG_TAGS },
    });
    expect(menuAxe.violations).toEqual([]);

    await user.click(screen.getByTestId("web-account-managed-homes"));
    const homes = await screen.findByRole("dialog", { name: "Managed Homes" });
    expect(
      await within(homes).findByText(/Managed Home plans are not available in this build/i),
    ).toBeInTheDocument();
    await user.click(within(homes).getByRole("button", { name: "Close drawer" }));

    await user.click(screen.getByTestId("web-account-button"));
    await user.click(screen.getByTestId("web-account-details"));
    const details = await screen.findByRole("dialog", { name: "Account details" });
    expect(within(details).getAllByText("owner@example.com").length).toBeGreaterThan(0);
    expect(within(details).getByTestId("cloud-account-panel")).toBeInTheDocument();
    await user.click(within(details).getByRole("button", { name: "Close drawer" }));

    await user.click(screen.getByTestId("web-account-button"));
    await user.click(screen.getByTestId("web-account-sign-out"));
    expect(screen.getByTestId("web-account-sign-in")).toHaveAttribute("href", "/cloud");
    expect(screen.queryByTestId("web-account-button")).not.toBeInTheDocument();
  });

  it("has no WCAG 2.2 AA axe violations", async () => {
    const { container } = await renderApp();
    await waitFor(() => {
      expect(screen.getByTestId("web-lanes")).toBeInTheDocument();
    });
    const results = await axe.run(container, {
      runOnly: { type: "tag", values: WCAG_TAGS },
    });
    expect(results.violations).toEqual([]);
  });

  it("has no WCAG 2.2 AA axe violations with the job drawer open", async () => {
    const user = userEvent.setup();
    await renderApp();
    await user.click(screen.getByTestId("job-card-ss-9-gate-check"));
    await screen.findByTestId("job-detail-fields");
    const results = await axe.run(document.body, {
      runOnly: { type: "tag", values: WCAG_TAGS },
    });
    expect(results.violations).toEqual([]);
  });
});
