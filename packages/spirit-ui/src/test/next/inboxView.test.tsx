import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import axe from "axe-core";
import { describe, expect, it, vi } from "vitest";

import { MockBridge } from "../../bridge/MockBridge";
import { InboxView, inboxKindLabel } from "../../next/InboxView";
import { seedStore } from "../render";

const WCAG_TAGS = ["wcag2a", "wcag2aa", "wcag21aa", "wcag22aa"];

async function seeded() {
  const bridge = new MockBridge("busy");
  const host = (await bridge.hosts.list())[0];
  if (host) await bridge.connection.connect(host.id);
  const fleet = await bridge.fleet.refresh();
  seedStore(fleet, { bridge });
  return bridge;
}

describe("InboxView", () => {
  it("maps kinds to Decision, Question and Notice", () => {
    expect(inboxKindLabel("question")).toBe("Question");
    expect(inboxKindLabel("notice")).toBe("Notice");
    expect(inboxKindLabel("decision")).toBe("Decision");
    expect(inboxKindLabel("retry")).toBe("Decision");
  });

  it("global scope shows Global and project items with filter chips", async () => {
    await seeded();
    render(<InboxView scope="all" />);
    const items = await screen.findAllByTestId("inbox-item");
    expect(items).toHaveLength(2);
    expect(screen.getByText("Raise the worker target to 6?")).toBeInTheDocument();
    expect(screen.getByText("Choose a merge queue policy")).toBeInTheDocument();
    const projects = screen.getAllByTestId("inbox-project").map((node) => node.textContent);
    expect(projects.sort()).toEqual(["Global", "vowp"]);
    // Options come from the full read, not the snapshot summary.
    expect(await screen.findByRole("button", { name: "Apply" })).toBeInTheDocument();

    const user = userEvent.setup();
    const filter = screen.getByRole("group", { name: "Filter inbox" });
    await user.click(within(filter).getByRole("button", { name: /^Global/ }));
    expect(screen.getAllByTestId("inbox-item")).toHaveLength(1);
    expect(screen.getByText("Raise the worker target to 6?")).toBeInTheDocument();
    await user.click(within(filter).getByRole("button", { name: /^vowp/ }));
    expect(screen.getAllByTestId("inbox-item")).toHaveLength(1);
    expect(screen.getByText("Choose a merge queue policy")).toBeInTheDocument();
  });

  it("project scope shows only that project's items", async () => {
    await seeded();
    render(<InboxView scope={{ project: "vowp" }} />);
    expect(await screen.findByText("Choose a merge queue policy")).toBeInTheDocument();
    expect(screen.queryByText("Raise the worker target to 6?")).not.toBeInTheDocument();
    expect(screen.queryByRole("group", { name: "Filter inbox" })).not.toBeInTheDocument();
    expect(screen.getByTestId("inbox-open-count")).toHaveTextContent("1");
  });

  it("one click on an option answers the item and confirms", async () => {
    const bridge = await seeded();
    const answer = vi.spyOn(bridge.fleet, "answerInbox");
    render(<InboxView scope="all" />);
    const user = userEvent.setup();
    await user.click(await screen.findByRole("button", { name: "Keep" }));
    expect(answer).toHaveBeenCalledWith("01MOCKINBOX000000000000000002", "Keep");
    await waitFor(() => expect(screen.queryByText("Raise the worker target to 6?")).not.toBeInTheDocument());
    expect(screen.getByTestId("inbox-confirmation")).toHaveTextContent("Keep");
    expect(screen.getAllByTestId("inbox-item")).toHaveLength(1);
  });

  it("a typed answer is sent through the bridge", async () => {
    const bridge = await seeded();
    const answer = vi.spyOn(bridge.fleet, "answerInbox");
    render(<InboxView scope={{ project: "vowp" }} />);
    const user = userEvent.setup();
    await user.type(await screen.findByLabelText("Answer for Choose a merge queue policy"), "Neither");
    await user.click(screen.getByRole("button", { name: "Send" }));
    expect(answer).toHaveBeenCalledWith("01MOCKINBOX000000000000000001", "Neither");
  });

  it("shows the calm empty state when nothing is open", async () => {
    const bridge = new MockBridge("busy");
    const host = (await bridge.hosts.list())[0];
    if (host) await bridge.connection.connect(host.id);
    const fleet = await bridge.fleet.refresh();
    seedStore({ ...fleet, inbox: [] }, { bridge });
    render(<InboxView scope="all" />);
    expect(screen.getByTestId("inbox-empty")).toHaveTextContent("All clear — nothing waiting for you");
  });

  it("has no WCAG 2.2 AA violations", async () => {
    await seeded();
    const { container } = render(<InboxView scope="all" />);
    await screen.findByRole("button", { name: "Apply" });
    const results = await axe.run(container, { runOnly: WCAG_TAGS });
    expect(results.violations.map((violation) => violation.id)).toEqual([]);
  });
});
