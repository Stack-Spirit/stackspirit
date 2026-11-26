import { describe, expect, it } from "vitest";

import type { OrchestratorView } from "../../bridge/wire";
import { resolveProjectOrchestrator, selectProjectOrchestrator } from "../../next/ChatView";

function card(name: string, app: string, project: string | null): OrchestratorView {
  return {
    name,
    session_id: `sess-${name}`,
    project,
    app,
    state: "idle",
    join: {} as OrchestratorView["join"],
    node: null,
  };
}

const opencode = card("local-stackspirit", "opencode", "stackspirit");
const chat = card("local-chat", "chat", "stackspirit");
const global = card("local-global", "chat", null);

describe("selectProjectOrchestrator", () => {
  it("prefers the chat card when opencode is listed first", () => {
    expect(selectProjectOrchestrator([opencode, chat], "stackspirit")).toBe(chat);
  });

  it("prefers the chat card when it is listed first", () => {
    expect(selectProjectOrchestrator([chat, opencode], "stackspirit")).toBe(chat);
  });

  it("falls back to the first match without a chat card", () => {
    expect(selectProjectOrchestrator([opencode], "stackspirit")).toBe(opencode);
  });

  it("returns null when no project matches", () => {
    expect(selectProjectOrchestrator([opencode, chat], "other")).toBeNull();
  });

  it("selects the project-less card for global", () => {
    expect(selectProjectOrchestrator([opencode, global], null)).toBe(global);
  });
});

describe("resolveProjectOrchestrator", () => {
  it("prefers an opencode card over a chat stub, in either order", () => {
    expect(resolveProjectOrchestrator([chat, opencode], "stackspirit")).toBe(opencode);
    expect(resolveProjectOrchestrator([opencode, chat], "stackspirit")).toBe(opencode);
  });

  it("prefers the canonical <project> card over local-<project>", () => {
    const canonical = card("stackspirit", "opencode", "stackspirit");
    expect(resolveProjectOrchestrator([chat, opencode, canonical], "stackspirit")).toBe(
      canonical,
    );
  });

  it("still returns a chat stub when it is the only match", () => {
    expect(resolveProjectOrchestrator([chat], "stackspirit")).toBe(chat);
  });
});
