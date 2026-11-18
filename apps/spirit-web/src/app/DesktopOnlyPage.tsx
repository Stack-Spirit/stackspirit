import { Badge, Button, Panel } from "@stackspirit/ui";

import {
  ENGINE_DESKTOP_ONLY,
  GATES_DESKTOP_ONLY,
  HOSTS_DESKTOP_ONLY,
  READ_ONLY_NOTICE,
} from "../copy";
import { SpiritMark } from "../shared/next";

export type DesktopSurface = "hosts" | "engine" | "gates";

const COPY: Record<DesktopSurface, { title: string; body: string }> = {
  hosts: { title: "Hosts are desktop-only", body: HOSTS_DESKTOP_ONLY },
  engine: { title: "Engine management is desktop-only", body: ENGINE_DESKTOP_ONLY },
  gates: { title: "Capability gates are desktop-only", body: GATES_DESKTOP_ONLY },
};

/**
 * A direct `/hosts`, `/engine`, or `/gates` load renders this shell without creating a
 * bridge at all, so the desktop-only page makes no daemon request. Returning to Fleet is a
 * full reload so the normal bridge bootstrap runs exactly once.
 */
export function DesktopOnlyBoot({ surface }: { surface: DesktopSurface }) {
  const copy = COPY[surface];
  const preview = import.meta.env.VITE_SPIRIT_MOCK === "1";
  return (
    <div className="app-shell web-shell">
      <a href="#main-canvas" className="skip-link">
        Skip to content
      </a>
      <header className="topbar web-topbar">
        <div className="topbar__brand">
          <SpiritMark size={28} />
          <span className="topbar__wordmark">StackSpirit</span>
          {preview ? (
            <Badge tone="warn" uppercase className="preview-badge">
              Preview data
            </Badge>
          ) : null}
        </div>
        <Badge tone="unknown" uppercase title={READ_ONLY_NOTICE}>
          read-only
        </Badge>
      </header>
      <div className="app-body">
        <aside className="rail rail--left" aria-label="Navigation">
          <nav className="view-nav" aria-label="Primary views">
            <button
              type="button"
              className="view-nav__item"
              onClick={() => window.location.assign("/")}
            >
              <span aria-hidden="true">◈</span>
              Fleet
            </button>
          </nav>
          <p className="notice notice--warn web-rail-notice">{READ_ONLY_NOTICE}</p>
        </aside>
        <main id="main-canvas" className="canvas" tabIndex={-1}>
          <div className="canvas__header">
            <div>
              <span className="eyebrow">Web cockpit</span>
              <h1 className="page-title">{copy.title}</h1>
            </div>
          </div>
          <div className="canvas__scroll">
            <div className="canvas__inner">
              <DesktopOnlyPage
                surface={surface}
                onBack={() => window.location.assign("/")}
              />
            </div>
          </div>
        </main>
      </div>
    </div>
  );
}

export function DesktopOnlyPage({
  surface,
  onBack,
}: {
  surface: DesktopSurface;
  onBack: () => void;
}) {
  const copy = COPY[surface];
  return (
    <Panel title={copy.title} tone="warn" data-testid={`desktop-only-${surface}`}>
      <p>{copy.body}</p>
      <p className="muted small">
        This route makes no bridge call. The Web shell has one daemon and one project.
      </p>
      <Button variant="secondary" onClick={onBack}>
        Back to Fleet
      </Button>
    </Panel>
  );
}
