import { useEffect, useState } from "react";

import type { SpiritBridge } from "../bridge";
import { Badge, Drawer } from "../components";
import { useFleetStore, usePreview } from "../state/fleetStore";
import { CapacityPlaceholder } from "../views/capacity/Placeholder";
import { EnginePlaceholder } from "../views/engine/Placeholder";
import { FleetPlaceholder } from "../views/fleet/Placeholder";
import { HostManager } from "../views/hosts/HostManager";
import { HostRail } from "../views/hosts/HostRail";
import { InboxDrawer, RawDocumentDrawer } from "./Drawers";
import { StatusStrip } from "./StatusStrip";
import { TopBar, type AppView } from "./TopBar";

const VIEWS: Array<{ id: AppView; label: string; icon: string }> = [
  { id: "fleet", label: "Fleet", icon: "◈" },
  { id: "engine", label: "Engine", icon: "◇" },
  { id: "hosts", label: "Hosts", icon: "⌁" },
];

export function AppShell({ bridge }: { bridge: SpiritBridge }) {
  const initialize = useFleetStore((state) => state.initialize);
  const preview = usePreview();
  const [view, setView] = useState<AppView>("fleet");
  const [inboxOpen, setInboxOpen] = useState(false);
  const [rawOpen, setRawOpen] = useState(false);
  const [capacityOpen, setCapacityOpen] = useState(false);

  useEffect(() => {
    let disposed = false;
    let unsubscribe: (() => void) | undefined;
    void initialize(bridge).then((cleanup) => {
      if (disposed) cleanup();
      else unsubscribe = cleanup;
    });
    return () => {
      disposed = true;
      unsubscribe?.();
    };
  }, [bridge, initialize]);

  return (
    <div className="app-shell" data-preview={preview ? "true" : "false"}>
      <a href="#main-canvas" className="skip-link">
        Skip to content
      </a>
      <TopBar
        onNavigate={setView}
        onOpenInbox={() => setInboxOpen(true)}
        onOpenCapacity={() => setCapacityOpen(true)}
      />

      <div className="app-body">
        <aside className="rail rail--left" aria-label="Hosts and navigation">
          <nav className="view-nav" aria-label="Primary views">
            {VIEWS.map((item) => (
              <button
                key={item.id}
                type="button"
                className={`view-nav__item${view === item.id ? " view-nav__item--active" : ""}`}
                aria-current={view === item.id ? "page" : undefined}
                onClick={() => setView(item.id)}
              >
                <span aria-hidden="true">{item.icon}</span>
                {item.label}
              </button>
            ))}
          </nav>
          <HostRail onManage={() => setView("hosts")} />
        </aside>

        <main id="main-canvas" className="canvas" tabIndex={-1}>
          <div className="canvas__header">
            <div>
              <span className="eyebrow">Cockpit</span>
              <h1 className="page-title">
                {view === "fleet" ? "Fleet" : view === "engine" ? "Engine" : "Hosts"}
              </h1>
            </div>
            <div className="canvas__header-actions">
              {preview ? <Badge tone="warn">Preview data</Badge> : null}
              <button
                type="button"
                className="btn btn--secondary btn--sm canvas__capacity-toggle"
                onClick={() => setCapacityOpen(true)}
              >
                Capacity
              </button>
            </div>
          </div>

          {view === "fleet" ? <FleetPlaceholder onOpenHosts={() => setView("hosts")} /> : null}
          {view === "engine" ? <EnginePlaceholder /> : null}
          {view === "hosts" ? <HostManager /> : null}
        </main>

        <aside
          className="rail rail--right"
          aria-label="Capacity and storage"
          tabIndex={0}
        >
          <CapacityPlaceholder onOpenHosts={() => setView("hosts")} />
        </aside>
      </div>

      <StatusStrip onOpenRaw={() => setRawOpen(true)} />

      <InboxDrawer open={inboxOpen} onClose={() => setInboxOpen(false)} />
      <RawDocumentDrawer open={rawOpen} onClose={() => setRawOpen(false)} />

      {capacityOpen ? (
        <Drawer
          open
          title="Capacity & VPS Storage"
          onClose={() => setCapacityOpen(false)}
        >
          <CapacityPlaceholder onOpenHosts={() => setView("hosts")} />
        </Drawer>
      ) : null}
    </div>
  );
}
