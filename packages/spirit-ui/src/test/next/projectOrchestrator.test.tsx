import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it } from "vitest";

import { MockBridge } from "../../bridge/MockBridge";
import type { SpiritBridge } from "../../bridge/SpiritBridge";
import { resolveProjectOrchestrator } from "../../next/ChatView";
import { resetConversations } from "../../next/conversations";
import { NextShell } from "../../next/NextShell";
import { resetShellUi } from "../../next/shellUi";
import { resetFleetStore, useFleetStore } from "../../state/fleetStore";
import type { OrchestratorView } from "../../bridge/wire";

const bridges: MockBridge[] = [];

async function connect(bridge: MockBridge) {
  resetFleetStore();
  resetConversations();
  resetShellUi();
  const user = userEvent.setup();
  const view = render(<NextShell bridge={bridge as SpiritBridge} />);
  await waitFor(() => expect(useFleetStore.getState().hostsLoaded).toBe(true));
  const prompt = screen.queryByTestId("connect-prompt-connect");
  if (prompt) await user.click(prompt);
  await waitFor(() => expect(useFleetStore.getState().connection.state).toBe("connected"));
  return { user, view };
}

async function openOrchestrator(user: ReturnType<typeof userEvent.setup>) {
  const sidebar = screen.getByRole("navigation", { name: "Sessions" });
  await user.click(within(sidebar).getByRole("button", { name: /^vowp/ }));
  await user.click(within(sidebar).getByRole("button", { name: /^Orchestrator/ }));
}

function row(name: string, project: string | null, state: string): OrchestratorView {
  return {
    name,
    session_id: "01JCF5WQ8ZK3M2V7N4P6R8T0AE",
    project,
    app: "opencode",
    state,
    join: { desktop: "d", phone: "p", chat: "c" },
    node: "local-mac",
  } as OrchestratorView;
}

afterEach(() => {
  for (const bridge of bridges.splice(0)) bridge.dispose?.();
  resetConversations();
  resetShellUi();
  resetFleetStore();
});

describe("project orchestrator discovery", () => {
  it("prefers running, then the canonical name, then a stopped project-bound card", () => {
    const running = row("local-vowp", "vowp", "running");
    const canonical = row("vowp", "vowp", "stopped");
    const bound = row("local-vowp", "vowp", "stopped");
    expect(resolveProjectOrchestrator([canonical, running], "vowp")).toBe(running);
    expect(resolveProjectOrchestrator([bound, canonical], "vowp")).toBe(canonical);
    expect(resolveProjectOrchestrator([row("x", "other", "running"), bound], "vowp")).toBe(bound);
    expect(resolveProjectOrchestrator([row("x", "other", "running")], "vowp")).toBeNull();
  });
});

describe("project orchestrator persistence", () => {
  it("shows 'not created' and Create & Start creates the canonical orchestrator once", async () => {
    const bridge = new MockBridge("chat-project-not-configured");
    bridges.push(bridge);
    const { user } = await connect(bridge);
    await openOrchestrator(user);

    const card = await screen.findByTestId("start-orchestrator");
    await waitFor(() =>
      expect(within(card).getByTestId("project-orchestrator-state")).toHaveTextContent("not created"),
    );
    await user.click(within(card).getByRole("button", { name: "Create & Start vowp orchestrator" }));
    await waitFor(() => expect(screen.queryByTestId("start-orchestrator")).not.toBeInTheDocument());

    const rows = await bridge.localOrchestrators!.orchestratorList();
    const mine = rows.filter((r) => r.project === "vowp");
    expect(mine).toHaveLength(1);
    expect(mine[0]).toMatchObject({ name: "vowp", state: "running" });
  });

  it("offers Resume for a stopped saved orchestrator instead of creating a duplicate", async () => {
    const bridge = new MockBridge("chat-project-not-configured");
    bridges.push(bridge);
    const { user } = await connect(bridge);
    const local = bridge.localOrchestrators!;
    await local.orchestratorNew({ node: "local-mac", app: "opencode", project: "vowp", name: "local-vowp" });
    await local.orchestratorStop("local-vowp");
    await openOrchestrator(user);

    const card = await screen.findByTestId("start-orchestrator");
    await waitFor(() =>
      expect(within(card).getByTestId("project-orchestrator-state")).toHaveTextContent("stopped"),
    );
    expect(within(card).queryByRole("button", { name: /Create & Start/ })).not.toBeInTheDocument();
    await user.click(within(card).getByRole("button", { name: "Resume vowp orchestrator" }));
    await waitFor(() => expect(screen.queryByTestId("start-orchestrator")).not.toBeInTheDocument());

    const rows = (await local.orchestratorList()).filter((r) => r.project === "vowp");
    expect(rows).toHaveLength(1);
    expect(rows[0]).toMatchObject({ name: "local-vowp", state: "running" });
  });

  it("reconnects after reopening: the saved orchestrator is found, not recreated", async () => {
    const bridge = new MockBridge("chat-project-not-configured");
    bridges.push(bridge);
    const first = await connect(bridge);
    await openOrchestrator(first.user);
    const card = await screen.findByTestId("start-orchestrator");
    await first.user.click(within(card).getByRole("button", { name: /Create & Start/ }));
    await waitFor(() => expect(screen.queryByTestId("start-orchestrator")).not.toBeInTheDocument());
    first.view.unmount();

    // Reopen StackSpirit against the same VOWP home.
    resetConversations();
    resetShellUi();
    const second = await connect(bridge);
    await openOrchestrator(second.user);
    await waitFor(() =>
      expect(screen.getByRole("textbox", { name: "Message" })).toBeEnabled(),
    );
    const rows = (await bridge.localOrchestrators!.orchestratorList()).filter(
      (r) => r.project === "vowp",
    );
    expect(rows).toHaveLength(1);
  });
});
