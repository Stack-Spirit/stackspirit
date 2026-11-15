import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import axe from "axe-core";
import { beforeEach, describe, expect, it } from "vitest";

import { WebMockBridge } from "../bridge/WebMockBridge";
import {
  CAPACITY_DETAIL_EXPLAIN,
  CAPACITY_DETAIL_UNAVAILABLE,
  ENGINE_DESKTOP_ONLY,
  GATE_FAILURES_UNAVAILABLE,
  HOSTS_DESKTOP_ONLY,
} from "../copy";
import { WebApp } from "./WebApp";

const WCAG_TAGS = ["wcag2a", "wcag2aa", "wcag21aa", "wcag22aa"];

async function renderApp(scenario: "fresh" | "stale-cache" = "fresh") {
  const bridge = new WebMockBridge(scenario);
  const result = render(<WebApp bridge={bridge} />);
  await screen.findByTestId("web-lanes");
  return { bridge, ...result };
}

beforeEach(() => {
  window.history.replaceState(null, "", "/");
});

describe("Web read-only shell", () => {
  it("renders the preview board, lanes, running cards, and the persistent badge", async () => {
    await renderApp();
    expect(screen.getByRole("heading", { name: "Fleet", level: 1 })).toBeInTheDocument();
    expect(screen.getAllByText("Preview data").length).toBeGreaterThan(0);
    expect(screen.getByTestId("web-lanes")).toBeInTheDocument();
    expect(screen.getByTestId("running-workers")).toBeInTheDocument();
    expect(screen.getByTestId("lane-count-ready")).toHaveTextContent("3");
    expect(screen.getByTestId("lane-review_fix")).toBeInTheDocument();
    expect(screen.getByTestId("daemon-panel")).toBeInTheDocument();
    expect(screen.getAllByText("read-only").length).toBeGreaterThan(0);
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

  it("shows the review/fix count and the gate-failure notice instead of an empty lane", async () => {
    await renderApp();
    const lane = screen.getByTestId("lane-review_fix");
    expect(within(lane).getByTestId("lane-count-review_fix")).toHaveTextContent("1");
    expect(within(lane).getByTestId("gate-failures-unavailable")).toHaveTextContent(
      GATE_FAILURES_UNAVAILABLE,
    );
    expect(lane.textContent).not.toMatch(/No jobs/i);
  });

  it("keeps Hosts and Engine out of the navigation and shows exact desktop-only copy on direct URLs", async () => {
    await renderApp();
    const nav = screen.getByRole("navigation", { name: "Primary views" });
    const buttons = within(nav).getAllByRole("button");
    expect(buttons).toHaveLength(1);
    expect(buttons[0]).toHaveTextContent("Fleet");

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
});
