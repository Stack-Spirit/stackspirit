import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it } from "vitest";

import { MockBridge } from "../../bridge/MockBridge";
import { MASKED_TEXT, MarkdownPreview, RemoteFilesView, SEATBELT_TEXT } from "../../next/RemoteFilesView";
import { resetFleetStore } from "../../state/fleetStore";
import { seedStore } from "../render";

async function mount(initialPath = "~") {
  const bridge = new MockBridge("fresh");
  const host = (await bridge.hosts.list())[0];
  if (host) await bridge.connection.connect(host.id);
  const fleet = await bridge.fleet.refresh();
  seedStore(fleet, { bridge });
  const view = render(<RemoteFilesView initialPath={initialPath} />);
  await screen.findByRole("table");
  return { bridge, fleet, ...view };
}

afterEach(() => resetFleetStore());

describe("MockBridge virtual filesystem", () => {
  it("enforces the same rules as the host", async () => {
    const bridge = new MockBridge("fresh");
    await expect(bridge.files.list("~")).rejects.toMatchObject({ code: "conflict" });
    const host = (await bridge.hosts.list())[0];
    if (host) await bridge.connection.connect(host.id);
    await bridge.fleet.refresh();
    const home = await bridge.files.list("~");
    expect(home.path).toBe("/home/owner");
    expect(home.entries.map((entry) => entry.name)).toEqual(expect.arrayContaining(["stackspirit", "vowp", ".vowp", "notes.md"]));
    const vowp = await bridge.files.list("~/.vowp");
    expect(vowp.entries.map((entry) => entry.name)).toEqual(expect.arrayContaining(["vowp.db", "vowp.toml", "logs", "backups", "worktrees"]));
    await expect(bridge.files.delete({ path: "~/.vowp/vowp.db" })).rejects.toMatchObject({ message: expect.stringMatching(/^protected_path/) });
    await expect(bridge.files.delete({ path: "~" , confirm_protected: true })).rejects.toMatchObject({ code: "refused" });
    expect((await bridge.files.preview("~/stackspirit/.env")).masked).toBe(true);
    expect((await bridge.files.preview("~/stackspirit/.env", true)).text).toContain("EXAMPLE_VALUE");
  });
});

describe("RemoteFilesView", () => {
  it("browses Home, projects, VOWP Home and Root from the locations list", async () => {
    const { fleet } = await mount();
    const user = userEvent.setup();
    const nav = screen.getByRole("navigation", { name: "Locations" });
    expect(screen.getByRole("table", { name: /\/home\/owner/ })).toBeInTheDocument();
    expect(within(nav).getByText("worker checkouts")).toBeInTheDocument();
    expect(within(nav).getByText("live database")).toBeInTheDocument();

    await user.click(within(nav).getByRole("button", { name: /VOWP Home/ }));
    await screen.findByRole("table", { name: /\.vowp/ });
    expect(screen.getByTestId("file-row-vowp.db")).toBeInTheDocument();
    expect(screen.getByTestId("file-row-vowp.db")).toHaveTextContent("Live VOWP path");

    const project = fleet.projects[0]?.name;
    expect(project).toBeTruthy();
    await user.click(within(nav).getByRole("button", { name: project as string }));
    await screen.findByRole("table", { name: new RegExp(`/home/owner/${project}`) });

    await user.click(within(nav).getByRole("button", { name: /Root/ }));
    await screen.findByRole("table", { name: /Contents of \/$/ });
    expect(screen.getByTestId("file-row-home")).toBeInTheDocument();
  });

  it("previews a file and masks a secret-like file until revealed", async () => {
    await mount("~/stackspirit");
    const user = userEvent.setup();
    await user.click(within(screen.getByTestId("file-row-README.md")).getByRole("button", { name: "README.md" }));
    const preview = await screen.findByTestId("file-preview");
    await within(preview).findByText("Desktop control room for VOWP.");

    await user.click(within(screen.getByTestId("file-row-.env")).getByRole("button", { name: ".env" }));
    expect(await screen.findByTestId("masked-preview")).toHaveTextContent(MASKED_TEXT);
    expect(screen.queryByText(/EXAMPLE_VALUE/)).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Reveal" }));
    expect(await screen.findByText(/EXAMPLE_VALUE/)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Hide" }));
    await waitFor(() => expect(screen.queryByText(/EXAMPLE_VALUE/)).not.toBeInTheDocument());
  });

  it("creates a folder, renames it and moves it", async () => {
    const { bridge } = await mount();
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "New folder" }));
    await user.type(screen.getByLabelText("Folder name"), "scratch");
    await user.click(screen.getByRole("button", { name: "Create" }));
    await screen.findByTestId("file-row-scratch");

    await user.click(screen.getByRole("button", { name: "Actions for scratch" }));
    await user.click(screen.getByRole("menuitem", { name: "Rename" }));
    const name = screen.getByLabelText("New name");
    await user.clear(name);
    await user.type(name, "archive");
    await user.click(screen.getByRole("button", { name: "Rename" }));
    await screen.findByTestId("file-row-archive");
    expect(screen.queryByTestId("file-row-scratch")).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Actions for archive" }));
    await user.click(screen.getByRole("menuitem", { name: "Move…" }));
    const destination = screen.getByLabelText("Destination folder");
    await user.clear(destination);
    await user.type(destination, "~/vowp");
    await user.click(screen.getByRole("button", { name: "Move" }));
    await waitFor(() => expect(screen.queryByTestId("file-row-archive")).not.toBeInTheDocument());
    expect((await bridge.files.list("~/vowp")).entries.map((entry) => entry.name)).toContain("archive");
  });

  it("moves a row dropped onto a folder", async () => {
    const { bridge } = await mount();
    const note = screen.getByTestId("file-row-notes.md");
    const folder = screen.getByTestId("file-row-vowp");
    fireEvent.dragStart(note);
    fireEvent.dragOver(folder);
    fireEvent.drop(folder);
    await waitFor(() => expect(screen.queryByTestId("file-row-notes.md")).not.toBeInTheDocument());
    expect((await bridge.files.list("~/vowp")).entries.map((entry) => entry.name)).toContain("notes.md");
  });

  it("asks for the seatbelt before deleting vowp.db and does nothing on cancel", async () => {
    const { bridge } = await mount("~/.vowp");
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Actions for vowp.db" }));
    await user.click(screen.getByRole("menuitem", { name: "Delete" }));
    await user.click(screen.getByRole("button", { name: "Delete" }));
    expect(await screen.findByTestId("seatbelt-text")).toHaveTextContent(SEATBELT_TEXT);
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect((await bridge.files.list("~/.vowp")).entries.map((entry) => entry.name)).toContain("vowp.db");

    await user.click(screen.getByRole("button", { name: "Actions for vowp.db" }));
    await user.click(screen.getByRole("menuitem", { name: "Delete" }));
    await user.click(screen.getByRole("button", { name: "Delete" }));
    await user.click(await screen.findByRole("button", { name: "Continue anyway" }));
    await waitFor(() => expect(screen.queryByTestId("file-row-vowp.db")).not.toBeInTheDocument());
  });

  it("renders Markdown without injecting HTML", () => {
    render(<MarkdownPreview source={"# Title\n\n- one **bold**\n- `code`\n\n<script>alert(1)</script>\n"} />);
    expect(screen.getByRole("heading", { name: "Title" })).toBeInTheDocument();
    expect(screen.getByText("bold").tagName).toBe("STRONG");
    expect(document.querySelector("script")).toBeNull();
    expect(screen.getByText("<script>alert(1)</script>")).toBeInTheDocument();
  });
});
