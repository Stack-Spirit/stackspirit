import { create } from "zustand";

import type { ChatPanelTarget, SessionId } from "../bridge/wire";

/** What fills the main area, to the right of the sidebar. */
export type NextView =
  | { kind: "session"; session: SessionId }
  /** Full page: server health, node map of projects, project rows. */
  | { kind: "server" }
  /** Full page: one project's workers and jobs as a node map and rows. */
  | { kind: "workers"; project: string }
  /** Full page: read-only live session of one worker, with explicit take over. */
  | { kind: "workerSession"; project: string; jobId: string; sessionId: string }
  /** Full page: every open inbox item, Global and per project. */
  | { kind: "inbox" }
  /** Full page: Finder-style file manager for the connected server. */
  | { kind: "files"; path?: string };

/** What the slide-in details panel shows. The bridge owns the target type. */
export type PanelContent = ChatPanelTarget;

/** An editable draft waiting for one orchestrator composer (e.g. from an inbox card). */
export interface ComposerDraft {
  session: SessionId;
  text: string;
  /** Bumps on every request so the same text can be applied twice. */
  nonce: number;
}

interface ShellUiState {
  composerDraft: ComposerDraft | null;
  setComposerDraft: (session: SessionId, text: string) => void;
  clearComposerDraft: () => void;
  /** null means the panel is closed. It is closed by default. */
  panel: PanelContent | null;
  openPanel: (content: PanelContent) => void;
  closePanel: () => void;
  togglePanel: () => void;
}

/**
 * Layout state for the next shell. The details panel works like DeepSeek Harness's
 * right sidebar: hidden until you open it, or until something is opened for you
 * (an agent reply, a node, a row).
 */
export const useShellUi = create<ShellUiState>((set, get) => ({
  panel: null,
  composerDraft: null,
  setComposerDraft: (session, text) =>
    set({
      composerDraft: {
        session,
        text,
        nonce: (get().composerDraft?.nonce ?? 0) + 1,
      },
    }),
  clearComposerDraft: () => set({ composerDraft: null }),
  openPanel: (content) => set({ panel: content }),
  closePanel: () => set({ panel: null }),
  togglePanel: () => set({ panel: get().panel ? null : { kind: "overview" } }),
}));

export function resetShellUi(): void {
  useShellUi.setState({ panel: null, composerDraft: null });
}
