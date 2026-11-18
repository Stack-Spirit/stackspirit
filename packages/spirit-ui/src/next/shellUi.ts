import { create } from "zustand";

import type { SessionId } from "./conversations";

/** What fills the main area, to the right of the sidebar. */
export type NextView =
  | { kind: "session"; session: SessionId }
  | { kind: "settings" }
  /** Full page: server health, node map of projects, project rows. */
  | { kind: "server" }
  /** Full page: one project's workers and jobs as a node map and rows. */
  | { kind: "workers"; project: string };

/** What the slide-in details panel shows. */
export type PanelContent =
  /** Default when opened from the toggle: what is running, inbox, server at a glance. */
  | { kind: "overview" }
  | { kind: "server" }
  | { kind: "inbox" }
  | { kind: "job"; project: string; jobId: string };

interface ShellUiState {
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
  openPanel: (content) => set({ panel: content }),
  closePanel: () => set({ panel: null }),
  togglePanel: () => set({ panel: get().panel ? null : { kind: "overview" } }),
}));

export function resetShellUi(): void {
  useShellUi.setState({ panel: null });
}
