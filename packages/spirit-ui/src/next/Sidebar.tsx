import { useState, type HTMLAttributes, type ReactNode } from "react";

import { useCluster, useConnection, useFleet, useFleetStore, useHosts, usePreview } from "../state/fleetStore";
import { clusterPooled } from "./cluster";
import type { SessionId } from "./conversations";
import { useServerHealth } from "./health";
import { Menu, MenuItem } from "./Menu";
import type { NextView } from "./shellUi";
import { SpiritMark } from "./SpiritMark";
import { openInbox, projectSummaries, type ProjectSummary } from "./summary";
import type { ThemePreference } from "./themePreference";

export type { NextView } from "./shellUi";

export interface SidebarProps {
  view: NextView;
  onNavigate: (view: NextView) => void;
  onAddServer: () => void;
  onAddProject: () => void;
  /** Opens the cluster form when at least two servers are saved. */
  onFormCluster: () => void;
  theme: ThemePreference;
  onTheme: (theme: ThemePreference) => void;
  resizeHandle: HTMLAttributes<HTMLDivElement>;
  resizing: boolean;
  /** Opens the raw VOWP document (diagnostics). */
  onOpenRaw: () => void;
}

/**
 * Left sidebar, laid out like DeepSeek Harness: logo row, the server card, the
 * global orchestrator, a Projects section with an add button, and one Settings
 * line at the bottom. 32px rows with 14px titles; the list fades out under the
 * footer instead of drawing a divider.
 */
export function Sidebar({
  view,
  onNavigate,
  onAddServer,
  onAddProject,
  onFormCluster,
  theme,
  onTheme,
  resizeHandle,
  resizing,
  onOpenRaw,
}: SidebarProps) {
  const fleet = useFleet();
  const connection = useConnection();
  const preview = usePreview();
  const connected = connection.state === "connected" || connection.state === "stale";
  const projects = projectSummaries(fleet);
  const globalInbox = openInbox(fleet).length;
  const current = (session: SessionId) => (view.kind === "session" && view.session === session ? "page" : undefined);

  return (
    <aside className={`nx-side${resizing ? " nx-side--resizing" : ""}`} aria-label="Sessions and servers">
      <div className="nx-side__brand">
        <SpiritMark size={26} />
        <span className="nx-side__name">StackSpirit</span>
        {preview ? (
          <span className="nx-pill" title="Demo data. Nothing here comes from a real server.">
            Preview
          </span>
        ) : null}
      </div>

      <div className="nx-side__scroll">
        <ServerCard view={view} onNavigate={onNavigate} onAddServer={onAddServer} onOpenRaw={onOpenRaw} onFormCluster={onFormCluster} />

        <nav className="nx-side__nav" aria-label="Sessions">
          {connected ? (
            <button
              type="button"
              className="nx-row-item"
              aria-current={current("global")}
              onClick={() => onNavigate({ kind: "session", session: "global" })}
            >
              <span className="nx-row-item__icon" aria-hidden="true">
                <OrchestratorIcon />
              </span>
              <span className="nx-row-item__text">Global orchestrator</span>
              {globalInbox > 0 ? (
                <span className="nx-count" aria-label={`${globalInbox} open in inbox`}>
                  {globalInbox}
                </span>
              ) : null}
            </button>
          ) : (
            <button
              type="button"
              className="nx-row-item"
              aria-current={current("setup")}
              onClick={() => onNavigate({ kind: "session", session: "setup" })}
            >
              <span className="nx-row-item__icon" aria-hidden="true">
                <OrchestratorIcon />
              </span>
              <span className="nx-row-item__text">Setup assistant</span>
              <span className="nx-chip nx-chip--local nx-chip--tiny">Local</span>
            </button>
          )}

          <div className="nx-section">
            <span className="nx-section__label">Projects</span>
            <button
              type="button"
              className="nx-icon-btn nx-icon-btn--sm"
              aria-label="Add project"
              title={connected ? "Add project" : "Connect a server to add projects"}
              disabled={!connected}
              onClick={onAddProject}
            >
              <PlusIcon />
            </button>
          </div>

          {!connected ? (
            <p className="nx-side__empty">Connect a server to see its projects.</p>
          ) : projects.length === 0 ? (
            <p className="nx-side__empty">No projects yet. Add one with ＋.</p>
          ) : (
            projects.map((project) => (
              <ProjectNode key={project.name} project={project} view={view} onNavigate={onNavigate} />
            ))
          )}
        </nav>
      </div>
      <span className="nx-side__fade" aria-hidden="true" />

      <div className="nx-side__foot">
        <Menu
          label="Settings"
          placement="above"
          trigger={(props) => (
            <button type="button" className="nx-settings-trigger" {...props} data-testid="settings-trigger">
              <span className="nx-row-item__icon" aria-hidden="true">
                <GearIcon />
              </span>
              <span className="nx-row-item__text">Settings</span>
            </button>
          )}
        >
          {(close) => (
            <>
              <MenuItem
                icon={<PlusIcon />}
                onSelect={() => {
                  close();
                  onAddServer();
                }}
              >
                Add server
              </MenuItem>
              <div className="nx-menu__group" role="group" aria-label="Theme">
                <span className="nx-menu__icon" aria-hidden="true">
                  <ThemeIcon />
                </span>
                <span className="nx-menu__text">Theme</span>
                <span className="nx-segment nx-segment--tiny">
                  {(["system", "light", "dark"] as const).map((value) => (
                    <button
                      key={value}
                      type="button"
                      role="menuitemradio"
                      aria-checked={theme === value}
                      onClick={() => onTheme(value)}
                    >
                      {value === "system" ? "Auto" : value === "light" ? "Light" : "Dark"}
                    </button>
                  ))}
                </span>
              </div>
              <MenuItem
                icon={<GearIcon />}
                onSelect={() => {
                  close();
                  onNavigate({ kind: "settings" });
                }}
              >
                All settings
              </MenuItem>
            </>
          )}
        </Menu>
      </div>

      <div className="nx-resizer nx-resizer--side" aria-label="Resize sidebar" {...resizeHandle} />
    </aside>
  );
}

/**
 * The server at the top of the sidebar. Connected: one load number (the higher of
 * RAM and CPU), active workers and free disk; it opens the server page. Not
 * connected: the Connect button lives here. The ⋯ menu switches servers.
 */
function ServerCard({
  view,
  onNavigate,
  onAddServer,
  onOpenRaw,
  onFormCluster,
}: {
  view: NextView;
  onNavigate: (view: NextView) => void;
  onAddServer: () => void;
  onOpenRaw: () => void;
  onFormCluster: () => void;
}) {
  const hosts = useHosts();
  const clusterAvailable = useFleetStore((state) => Boolean(state.bridge?.cluster));
  const openSessions = useFleetStore((state) => state.openSessions);
  const sessionsAvailable = useFleetStore((state) => Boolean(state.bridge?.sessions));
  const connection = useConnection();
  const selectedHostId = useFleetStore((state) => state.selectedHostId);
  const connect = useFleetStore((state) => state.connect);
  const disconnect = useFleetStore((state) => state.disconnect);
  const health = useServerHealth();
  const cluster = useCluster();

  const host =
    hosts.find((item) => item.id === connection.host_id) ??
    hosts.find((item) => item.id === selectedHostId) ??
    hosts[0] ??
    null;

  if (!host) {
    return (
      <div className="nx-server nx-server--empty">
        <span className="nx-server__muted">No server yet</span>
        <button type="button" className="nx-btn nx-btn--sm" onClick={onAddServer}>
          Add server
        </button>
      </div>
    );
  }

  const mine = connection.host_id === host.id;
  const state = mine ? connection.state : "disconnected";
  const live = state === "connected" || state === "stale";
  const busy = state === "connecting" || state === "disconnecting";
  const others = hosts.filter((item) => item.id !== host.id);
  const loadPct = health.load ? Math.round(health.load.ratio * 100) : null;
  const loadTone = loadPct == null ? "" : loadPct >= 90 ? " nx-meter__fill--danger" : loadPct >= 75 ? " nx-meter__fill--warn" : "";
  const clusterHealth = cluster?.health ?? null;
  // The brief's badge appears only when at least one worker is actually online; an
  // all-offline cluster keeps the single-server card and never shows a pooled total.
  const pooled =
    clusterHealth && clusterHealth.online_workers > 0 ? clusterPooled(clusterHealth) : null;
  const clusterLoadPct =
    pooled && pooled.pooled > 0
      ? Math.min(100, Math.round((pooled.busy / pooled.pooled) * 100))
      : pooled
        ? 0
        : null;
  const clusterTone =
    clusterLoadPct == null
      ? ""
      : clusterLoadPct >= 90
        ? " nx-meter__fill--danger"
        : clusterLoadPct >= 75
          ? " nx-meter__fill--warn"
          : "";

  const menu = (
    <Menu
      label={`${host.label} options`}
      align="end"
      trigger={(props) => (
        <button type="button" className="nx-icon-btn nx-icon-btn--sm nx-server__more" aria-label="Server options" {...props}>
          <MoreIcon />
        </button>
      )}
    >
      {(close) => (
        <>
          {hosts.length >= 2 && clusterAvailable ? (
            <MenuItem
              icon={<ClusterIcon />}
              onSelect={() => {
                close();
                onFormCluster();
              }}
            >
              Form a cluster
            </MenuItem>
          ) : null}
          {live ? (
            <>
              <MenuItem
                onSelect={() => {
                  close();
                  onNavigate({ kind: "server" });
                }}
              >
                Open server page
              </MenuItem>
              {sessionsAvailable ? (
                <MenuItem
                  onSelect={() => {
                    close();
                    openSessions("list");
                  }}
                >
                  Sessions and .asif export
                </MenuItem>
              ) : null}
              <MenuItem
                onSelect={() => {
                  close();
                  onOpenRaw();
                }}
              >
                Raw VOWP document
              </MenuItem>
            </>
          ) : null}
          <MenuItem
            onSelect={() => {
              close();
              void (live || busy ? disconnect(host.id) : connect(host.id));
            }}
          >
            {busy ? "Cancel" : live ? `Disconnect ${host.label}` : `Connect ${host.label}`}
          </MenuItem>
          {others.map((other) => (
            <MenuItem
              key={other.id}
              meta={other.ssh.host}
              onSelect={() => {
                close();
                void connect(other.id);
              }}
            >
              Switch to {other.label}
            </MenuItem>
          ))}
          <MenuItem
            icon={<PlusIcon />}
            onSelect={() => {
              close();
              onAddServer();
            }}
          >
            Add server
          </MenuItem>
        </>
      )}
    </Menu>
  );

  if (!live) {
    return (
      <div className="nx-server" data-testid="server-card">
        <div className="nx-server__head">
          <span className={`nx-dot ${state === "failed" ? "nx-dot--danger" : busy ? "nx-dot--busy" : ""}`} aria-hidden="true" />
          <span className="nx-server__name">{host.label}</span>
          {menu}
        </div>
        <span className="nx-server__muted">
          {state === "connecting"
            ? "Connecting…"
            : state === "disconnecting"
              ? "Disconnecting…"
              : state === "failed"
                ? "Couldn't connect"
                : `${host.ssh.user}@${host.ssh.host}`}
        </span>
        <button
          type="button"
          className={`nx-btn nx-btn--sm nx-btn--block${busy ? " nx-btn--quiet" : ""}`}
          onClick={() => void (busy ? disconnect(host.id) : connect(host.id))}
          data-testid="sidebar-connect"
        >
          {busy ? "Cancel" : "Connect"}
        </button>
      </div>
    );
  }

  if (pooled) {
    return (
      <div className="nx-server" data-testid="server-card" data-cluster="true">
        <button
          type="button"
          className="nx-server__main"
          aria-current={view.kind === "server" ? "page" : undefined}
          aria-label={`${host.label}: cluster ${pooled.busy} of ${pooled.pooled} workers busy, ${pooled.onlineWorkers} workers online, ${health.diskShort}. Open server page`}
          onClick={() => onNavigate({ kind: "server" })}
          data-testid="health-card"
        >
          <span className="nx-server__head">
            <span className={`nx-dot ${health.stale ? "nx-dot--warn" : "nx-dot--ok"}`} aria-hidden="true" />
            <span className="nx-server__name">{host.label}</span>
            <span className="nx-chip nx-chip--cluster nx-chip--tiny" data-testid="cluster-badge">
              cluster
            </span>
          </span>
          <span className="nx-server__load">
            <span className="nx-server__load-label">Load</span>
            <span className="nx-meter__track">
              <span
                className={`nx-meter__fill${clusterTone}`}
                style={{ width: `${clusterLoadPct ?? 0}%` }}
              />
            </span>
            <span className="nx-server__load-value">
              {clusterLoadPct ?? 0}%<small>cluster</small>
            </span>
          </span>
          <span className="nx-server__facts">
            <span title="Busy / pooled target workers">
              <strong>{pooled.busy}</strong>/{pooled.pooled} workers busy
            </span>
            <span>{health.diskShort}</span>
          </span>
        </button>
        {menu}
      </div>
    );
  }

  return (
    <div className="nx-server" data-testid="server-card">
      <button
        type="button"
        className="nx-server__main"
        aria-current={view.kind === "server" ? "page" : undefined}
        aria-label={`${host.label}: load ${loadPct ?? "unknown"}%, ${health.running} of ${health.target ?? "unknown"} workers active, ${health.diskShort}. Open server page`}
        onClick={() => onNavigate({ kind: "server" })}
        data-testid="health-card"
      >
        <span className="nx-server__head">
          <span className={`nx-dot ${health.stale ? "nx-dot--warn" : "nx-dot--ok"}`} aria-hidden="true" />
          <span className="nx-server__name">{host.label}</span>
        </span>
        <span className="nx-server__load">
          <span className="nx-server__load-label">Load</span>
          <span className="nx-meter__track">
            <span className={`nx-meter__fill${loadTone}`} style={{ width: `${loadPct ?? 0}%` }} />
          </span>
          <span className="nx-server__load-value">
            {loadPct == null ? "–" : `${loadPct}%`}
            {health.load ? <small>{health.load.source}</small> : null}
          </span>
        </span>
        <span className="nx-server__facts">
          <span>
            <strong>{health.running}</strong>/{health.target ?? "?"} workers
          </span>
          <span>{health.diskShort}</span>
        </span>
      </button>
      {menu}
    </div>
  );
}

function ProjectNode({
  project,
  view,
  onNavigate,
}: {
  project: ProjectSummary;
  view: NextView;
  onNavigate: (view: NextView) => void;
}) {
  const session: SessionId = `project:${project.name}`;
  const inSession = view.kind === "session" && view.session === session;
  const inWorkers = view.kind === "workers" && view.project === project.name;
  const inProject = inSession || inWorkers;
  const [open, setOpen] = useState(inProject);
  // Expand when the project is opened from elsewhere (a node, a job), but let the
  // user collapse it again while inside.
  const [wasIn, setWasIn] = useState(inProject);
  if (inProject !== wasIn) {
    setWasIn(inProject);
    if (inProject) setOpen(true);
  }
  const paused = project.state !== "active";

  return (
    <div className="nx-proj">
      <button
        type="button"
        className="nx-proj__row"
        aria-expanded={open}
        data-current={inProject && !open ? "true" : undefined}
        onClick={() => setOpen((value) => !value)}
      >
        <span className="nx-proj__glyph" aria-hidden="true">
          <FolderIcon className="nx-proj__folder" />
          <span className={`nx-proj__chevron${open ? " nx-proj__chevron--open" : ""}`}>
            <ChevronIcon />
          </span>
        </span>
        <span className="nx-row-item__text">{project.name}</span>
        {paused ? <span className="nx-row-item__meta">{project.state}</span> : null}
        {project.inbox > 0 ? (
          <span className="nx-count" aria-label={`${project.inbox} open in inbox`}>
            {project.inbox}
          </span>
        ) : project.running > 0 ? (
          <span className="nx-row-item__meta" title={`${project.running} running`}>
            <span className="nx-dot nx-dot--ok nx-dot--tiny" aria-hidden="true" /> {project.running}
          </span>
        ) : null}
      </button>
      {open ? (
        <div className="nx-proj__children">
          <button
            type="button"
            className="nx-row-item nx-row-item--child"
            aria-current={inSession ? "page" : undefined}
            onClick={() => onNavigate({ kind: "session", session })}
          >
            <span className="nx-row-item__text">Orchestrator</span>
          </button>
          <button
            type="button"
            className="nx-row-item nx-row-item--child"
            aria-current={inWorkers ? "page" : undefined}
            onClick={() => onNavigate({ kind: "workers", project: project.name })}
          >
            <span className="nx-row-item__text">Workers</span>
            {project.blocked ? (
              <span className="nx-dot nx-dot--warn nx-dot--tiny" title={`${project.blocked} need attention`} />
            ) : null}
            <span className="nx-row-item__meta">{project.running} running</span>
          </button>
        </div>
      ) : null}
    </div>
  );
}

/* ---------- 16px line icons (stroke follows currentColor) ---------- */

function Icon({ children, className }: { children: ReactNode; className?: string }) {
  return (
    <svg className={className} width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      {children}
    </svg>
  );
}

function PlusIcon() {
  return (
    <Icon>
      <path d="M8 3.5v9M3.5 8h9" />
    </Icon>
  );
}

function GearIcon() {
  return (
    <Icon>
      <circle cx="8" cy="8" r="2" />
      <path d="M8 1.8v1.6M8 12.6v1.6M14.2 8h-1.6M3.4 8H1.8M12.4 3.6l-1.1 1.1M4.7 11.3l-1.1 1.1M12.4 12.4l-1.1-1.1M4.7 4.7 3.6 3.6" />
    </Icon>
  );
}

function ThemeIcon() {
  return (
    <Icon>
      <circle cx="8" cy="8" r="5.5" />
      <path d="M8 2.5v11a5.5 5.5 0 0 0 0-11Z" fill="currentColor" stroke="none" />
    </Icon>
  );
}

function MoreIcon() {
  return (
    <Icon>
      <circle cx="3.5" cy="8" r="0.6" fill="currentColor" />
      <circle cx="8" cy="8" r="0.6" fill="currentColor" />
      <circle cx="12.5" cy="8" r="0.6" fill="currentColor" />
    </Icon>
  );
}

function FolderIcon({ className }: { className?: string }) {
  return (
    <Icon className={className}>
      <path d="M2 4.5c0-.6.4-1 1-1h3l1.3 1.4H13c.6 0 1 .4 1 1V12c0 .6-.4 1-1 1H3c-.6 0-1-.4-1-1Z" />
    </Icon>
  );
}

function ChevronIcon() {
  return (
    <svg width="16" height="16" viewBox="0 0 16 16" aria-hidden="true">
      <path d="M6 4.5 10 8l-4 3.5Z" fill="currentColor" />
    </svg>
  );
}

function OrchestratorIcon() {
  return (
    <Icon>
      <path d="M3 3.5h10c.6 0 1 .4 1 1v6c0 .6-.4 1-1 1H8.5L5.5 14v-2.5H3c-.6 0-1-.4-1-1v-6c0-.6.4-1 1-1Z" />
    </Icon>
  );
}

function ClusterIcon() {
  return (
    <Icon>
      <circle cx="8" cy="5" r="2.2" />
      <circle cx="3.6" cy="11.4" r="1.8" />
      <circle cx="12.4" cy="11.4" r="1.8" />
      <path d="M6.2 6.6 4.4 9.8M9.8 6.6l1.8 3.2M5.4 11.4h5.2" />
    </Icon>
  );
}
