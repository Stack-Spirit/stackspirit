import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import axe from "axe-core";
import { describe, expect, it, vi } from "vitest";

import { MockBridge } from "../bridge/MockBridge";
import { seedConnectedStale, seedStore } from "./render";
import { scenario } from "./scenarios/fixtures";
import { InboxDrawer } from "../views/inbox/InboxDrawer";

const WCAG_TAGS = ["wcag2a", "wcag2aa", "wcag21aa", "wcag22aa"];

async function connectedBridge(): Promise<MockBridge> {
  const bridge = new MockBridge("fresh");
  const host = (await bridge.hosts.list())[0];
  if (host) await bridge.connection.connect(host.id);
  return bridge;
}

describe("inbox drawer", () => {
  it("shows the full body and suggested answers", async () => {
    const fixture = scenario("fresh-single");
    const bridge = await connectedBridge();
    seedStore(fixture.fleet, { bridge });
    render(<InboxDrawer open onClose={() => undefined} />);
    await waitFor(() =>
      expect(screen.getByTestId("inbox-body")).toHaveTextContent(
        "Should the supervisor merge one job at a time or batch ready jobs?",
      ),
    );
    expect(
      screen.getByRole("button", { name: "One at a time" }),
    ).toBeInTheDocument();
  });

  it("treats an empty full read as authoritative over stale snapshot summaries", async () => {
    const fixture = scenario("fresh-single");
    const bridge = await connectedBridge();
    vi.spyOn(bridge.fleet, "inbox").mockResolvedValue([]);
    seedStore(fixture.fleet, { bridge });
    render(<InboxDrawer open onClose={() => undefined} />);

    expect(await screen.findByText("Inbox is clear")).toBeInTheDocument();
    expect(
      screen.queryByText("Choose the included model set"),
    ).not.toBeInTheDocument();
    expect(screen.queryByRole("list")).not.toBeInTheDocument();
    expect(screen.queryByTestId("inbox-body")).not.toBeInTheDocument();
  });

  it("answers through the bridge and refreshes without moving the job locally", async () => {
    const fixture = scenario("fresh-single");
    const bridge = await connectedBridge();
    const answer = vi.spyOn(bridge.fleet, "answerInbox");
    const refresh = vi.spyOn(bridge.fleet, "refresh");
    seedStore(fixture.fleet, { bridge });
    render(<InboxDrawer open onClose={() => undefined} />);
    const user = userEvent.setup();
    await user.click(await screen.findByRole("button", { name: "One at a time" }));
    expect(screen.getByText(/Answering refreshes the snapshot/)).toBeInTheDocument();
    await user.click(screen.getByTestId("inbox-answer-submit"));
    await waitFor(() => expect(answer).toHaveBeenCalledTimes(1));
    expect(answer.mock.calls[0]?.[1]).toBe("One at a time");
    await waitFor(() => expect(refresh).toHaveBeenCalled());
    expect(await screen.findByTestId("inbox-answered")).toHaveTextContent("answered");
  });

  it("keeps empty answers locally rejected", async () => {
    const fixture = scenario("fresh-single");
    const bridge = await connectedBridge();
    const answer = vi.spyOn(bridge.fleet, "answerInbox");
    seedStore(fixture.fleet, { bridge });
    render(<InboxDrawer open onClose={() => undefined} />);
    const submit = await screen.findByTestId("inbox-answer-submit");
    expect(submit).toBeDisabled();
    expect(answer).not.toHaveBeenCalled();
  });

  it("bounds answers by UTF-8 bytes, not UTF-16 code units", async () => {
    const fixture = scenario("fresh-single");
    const bridge = await connectedBridge();
    seedStore(fixture.fleet, { bridge });
    render(<InboxDrawer open onClose={() => undefined} />);
    const submit = await screen.findByTestId("inbox-answer-submit");
    expect(submit).toBeDisabled();
    const field = screen.getByRole("textbox", { name: /^Answer/ });

    // 4,000 two-byte characters are 8,000 UTF-8 bytes and fit the limit.
    fireEvent.input(field, { target: { value: "é".repeat(4_000) } });
    await waitFor(() =>
      expect(screen.getByText(/bytes\. Empty answers/)).toHaveTextContent(
        "8000 / 10000 bytes",
      ),
    );
    await waitFor(() =>
      expect(screen.getByTestId("inbox-answer-submit")).toBeEnabled(),
    );

    // 5,001 two-byte characters are 10,002 bytes and must be rejected locally.
    fireEvent.input(field, { target: { value: "é".repeat(5_001) } });
    await waitFor(() =>
      expect(screen.getByTestId("inbox-answer-submit")).toBeDisabled(),
    );
    expect(screen.getByText(/10002 \/ 10000 bytes/)).toBeInTheDocument();
  });

  it("disables answers while the snapshot is stale", async () => {
    const fixture = scenario("fresh-single");
    const bridge = await connectedBridge();
    seedConnectedStale(fixture.fleet, { bridge });
    render(<InboxDrawer open onClose={() => undefined} />);
    expect(await screen.findByTestId("inbox-disabled-reason")).toHaveTextContent(
      "Refresh before answering.",
    );
    expect(screen.getByTestId("inbox-answer-submit")).toBeDisabled();
  });

  it("has no WCAG 2.2 AA axe violations on the inbox drawer", async () => {
    const fixture = scenario("fresh-single");
    const bridge = await connectedBridge();
    seedStore(fixture.fleet, { bridge });
    render(<InboxDrawer open onClose={() => undefined} />);
    await screen.findByTestId("inbox-body");
    const results = await axe.run(document.body, {
      runOnly: { type: "tag", values: WCAG_TAGS },
    });
    expect(results.violations).toEqual([]);
  });
});
