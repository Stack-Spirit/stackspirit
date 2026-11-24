import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it } from "vitest";

import { MockBridge } from "../../bridge/MockBridge";
import type { SpiritBridge } from "../../bridge/SpiritBridge";
import { resetConversations } from "../../next/conversations";
import { NextShell } from "../../next/NextShell";
import { resetShellUi, useShellUi } from "../../next/shellUi";
import { resetFleetStore, useFleetStore } from "../../state/fleetStore";

const bridges: MockBridge[] = [];

async function openJobPanel() {
  resetFleetStore();
  resetConversations();
  const bridge = new MockBridge("fresh");
  bridges.push(bridge);
  const user = userEvent.setup();
  render(<NextShell bridge={bridge as SpiritBridge} />);
  await waitFor(() => expect(useFleetStore.getState().hostsLoaded).toBe(true));
  await user.click(screen.getByTestId("connect-prompt-connect"));
  await waitFor(() => expect(useFleetStore.getState().connection.state).toBe("connected"));
  const fleet = useFleetStore.getState().fleet;
  const worker = fleet?.running.find((card) => card.session_id);
  expect(worker).toBeTruthy();
  act(() => {
    useShellUi.getState().openPanel({ kind: "job", project: worker!.project, jobId: worker!.job_id });
  });
  return { user, bridge, worker: worker! };
}

afterEach(() => {
  for (const bridge of bridges.splice(0)) bridge.dispose?.();
  resetConversations();
  resetShellUi();
  resetFleetStore();
});

describe("worker session viewer", () => {
  it("opens the live stream from the job panel", async () => {
    const { user, bridge, worker } = await openJobPanel();
    const panel = await screen.findByTestId("details-panel");
    await user.click(await within(panel).findByRole("button", { name: "Open session" }));
    const view = await screen.findByTestId("worker-session");
    expect(within(view).getByTestId("session-status")).toHaveTextContent("Live · polling");
    expect(within(view).getByTestId("session-badge").textContent).toMatch(/·/);
    const list = await within(view).findByTestId("session-events");
    expect(within(list).getAllByRole("listitem").length).toBeGreaterThan(2);
    expect(bridge.tailPolls[0]).toMatchObject({ project: worker.project, sessionId: worker.session_id, afterSeq: null });
  });

  it("asks before taking over, cancel changes nothing", async () => {
    const { user, bridge } = await openJobPanel();
    const panel = await screen.findByTestId("details-panel");
    await user.click(await within(panel).findByRole("button", { name: "Open session" }));
    const view = await screen.findByTestId("worker-session");
    await user.click(within(view).getByRole("button", { name: "Stop & Take over" }));
    const dialog = screen.getByRole("alertdialog", { name: "Confirm take over" });
    expect(dialog).toHaveTextContent(
      "This pauses the autonomous worker, saves a handoff capsule, fences its writes, and hands the session to you. Take over?",
    );
    await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
    expect(bridge.takeovers).toEqual([]);
  });

  it("takes over after confirmation and shows next steps", async () => {
    const { user, bridge, worker } = await openJobPanel();
    const panel = await screen.findByTestId("details-panel");
    await user.click(await within(panel).findByRole("button", { name: "Open session" }));
    const view = await screen.findByTestId("worker-session");
    await user.click(within(view).getByRole("button", { name: "Stop & Take over" }));
    await user.click(within(screen.getByRole("alertdialog")).getByRole("button", { name: "Take over" }));
    const err = screen.queryByTestId("session-error");
    if (err) throw new Error(err.textContent ?? "err");
    const result = await screen.findByTestId("takeover-result");
    expect(result).toHaveTextContent("You now own this session");
    expect(bridge.takeovers).toEqual([worker.session_id]);
    expect(within(view).getByTestId("session-status")).toHaveTextContent("Taken over");
  });
});
