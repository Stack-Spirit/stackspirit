import { useEffect, useMemo, useState } from "react";

import type { JobCardView, SpiritBridge, SpiritError } from "@stackspirit/ui";
import {
  Badge,
  Button,
  Drawer,
  EmptyState,
  Panel,
  formatAge,
  formatClock,
  latencyLabel,
} from "@stackspirit/ui";

import { READ_ONLY_NOTICE } from "../copy";
import { webErrorTitle } from "../bridge/errors";
import { SpiritMark } from "../shared/next";
import { CapacityPanel, SupervisorCard } from "./CapacityPanel";
import { DesktopOnlyPage } from "./DesktopOnlyPage";
import { FleetStructure } from "./FleetStructure";
import { WebInboxDrawer } from "./InboxDrawer";
import { JobDrawer } from "./JobDrawer";
import { Lanes, RunningWorkers, UnavailableDetails } from "./Lanes";
import { WebStatusStrip } from "./StatusStrip";
import { useWebFleet } from "./useWebFleet";

export type WebPath = "fleet" | "hosts" | "engine" | "gates";

export function pathFor(pathname: string): WebPath {
  if (pathname === "/hosts" || pathname === "/hosts/") return "hosts";
  if (pathname === "/engine" || pathname === "/engine/") return "engine";
  if (pathname === "/gates" || pathname === "/gates/") return "gates";
  return "fleet";
}

function connectionLabel(connection: { state: string }): string {
  switch (connection.state) {
    case "connected":
      return "Connected";
    case "reconnecting":
      return "Reconnecting";
    case "stale":
      return "No current data";
    case "failed":
      return "Failed";
    case "connecting":
      return "Connecting…";
    default:
      return connection.state;
  }
}

function ErrorBanner({
  error,
  onDismiss,
  onRetry,
  retrying,
}: {
  error: SpiritError;
  onDismiss: () => void;
  onRetry: () => void;
  retrying: boolean;
}) {
  return (
    <div className="notice notice--danger web-error-banner" role="alert">
      <div>
        <strong>{webErrorTitle(error)}</strong>
        <p>{error.message}</p>
        {error.hint ? <p className="muted small">{error.hint}</p> : null}
      </div>
      <div className="web-error-banner__actions">
        <Button size="sm" variant="secondary" onClick={onRetry} loading={retrying}>
          Retry
        </Button>
        <Button size="sm" variant="ghost" onClick={onDismiss}>
          Dismiss
        </Button>
      </div>
    </div>
  );
}

export function WebApp({ bridge }: { bridge: SpiritBridge }) {
  const fleet = useWebFleet(bridge);
  const [path, setPath] = useState<WebPath>(() => pathFor(window.location.pathname));
  const [inboxOpen, setInboxOpen] = useState(false);
  const [capacityOpen, setCapacityOpen] = useState(false);
  const [selectedJobKey, setSelectedJobKey] = useState<string | null>(null);

  useEffect(() => {
    const onPop = () => setPath(pathFor(window.location.pathname));
    window.addEventListener("popstate", onPop);
    return () => window.removeEventListener("popstate", onPop);
  }, []);

  function navigate(next: WebPath): void {
    const target = next === "fleet" ? "/" : `/${next}`;
    window.history.pushState(null, "", target);
    setPath(next);
    window.scrollTo?.(0, 0);
  }

  const snapshot = fleet.snapshot;
  const selectedJob = useMemo<JobCardView | null>(() => {
    if (!snapshot || !selectedJobKey) return null;
    const [project, jobId] = selectedJobKey.split(":");
    if (!jobId) return null;
    for (const cards of [
      snapshot.lanes.running,
      snapshot.lanes.ready,
      snapshot.lanes.review_fix,
      snapshot.lanes.parked_blocked,
      snapshot.lanes.done,
    ]) {
      const card = cards.find(
        (candidate) => candidate.job_id === jobId && candidate.project === project,
      );
      if (card) return card;
    }
    return null;
  }, [snapshot, selectedJobKey]);

  const stale = snapshot?.stale === true || fleet.connection.state === "stale";
  const preview = bridge.preview;
  const inboxCount = snapshot?.inbox.filter((item) => item.state === "open").length ?? 0;
  const statusLabel = connectionLabel(fleet.connection);

  return (
    <div className="app-shell web-shell" data-preview={preview ? "true" : "false"}>
      <a href="#main-canvas" className="skip-link">
        Skip to content
      </a>

      <header className="topbar web-topbar">
        <div className="topbar__brand">
          <SpiritMark size={28} />
          <span className="topbar__wordmark">StackSpirit</span>
          {preview ? (
            <Badge
              tone="warn"
              uppercase
              className="preview-badge"
              title="Mock data. Nothing here comes from a real VPS."
            >
              Preview data
            </Badge>
          ) : null}
        </div>

        <div className="topbar__cluster topbar__cluster--host">
          <span
            className={`status-dot status-dot--${
              stale ? "warn" : fleet.connection.state === "connected" ? "ok" : "unknown"
            }`}
            data-testid="web-connection-dot"
            data-tone={stale ? "warn" : fleet.connection.state}
            aria-hidden="true"
          />
          <div className="topbar__host-text">
            <strong>{snapshot?.project.name ?? "One daemon"}</strong>
            <span className="muted mono small">
              {snapshot
                ? `${snapshot.daemon.node} · daemon ${snapshot.daemon.version}`
                : "Waiting for the daemon"}
            </span>
          </div>
          <span className="muted small" data-testid="web-connection-label">
            {statusLabel}
          </span>
        </div>

        <Badge tone="unknown" uppercase title={READ_ONLY_NOTICE}>
          read-only
        </Badge>

        <Button
          variant="secondary"
          size="sm"
          onClick={() => void fleet.refresh()}
          loading={fleet.refreshing}
          data-testid="web-refresh"
        >
          Refresh
        </Button>
        <Button
          variant="ghost"
          size="sm"
          onClick={() => setInboxOpen(true)}
          aria-label={`Inbox, ${inboxCount} open`}
          data-testid="web-inbox-button"
        >
          <span aria-hidden="true">◔</span> {inboxCount}
        </Button>
        <Button
          variant="ghost"
          size="sm"
          onClick={() => setCapacityOpen(true)}
          data-testid="web-capacity-button"
        >
          Capacity
        </Button>
      </header>

      <div className="app-body">
        <aside className="rail rail--left" aria-label="Navigation">
          <nav className="view-nav" aria-label="Primary views">
            <button
              type="button"
              className={`view-nav__item${path === "fleet" ? " view-nav__item--active" : ""}`}
              aria-current={path === "fleet" ? "page" : undefined}
              onClick={() => navigate("fleet")}
            >
              <span aria-hidden="true">◈</span>
              Fleet
            </button>
          </nav>
          <p className="notice notice--warn web-rail-notice" data-testid="read-only-notice">
            {READ_ONLY_NOTICE}
          </p>
          <p className="muted small">
            Hosts, Engine, and Gates are desktop-only and are not in this navigation.
          </p>
        </aside>

        <main id="main-canvas" className="canvas" tabIndex={-1}>
          <div className="canvas__header">
            <div>
              <span className="eyebrow">Web cockpit</span>
              <h1 className="page-title">
                {path === "fleet"
                  ? "Fleet"
                  : path === "hosts"
                    ? "Hosts"
                    : path === "engine"
                      ? "Engine"
                      : "Gates"}
              </h1>
            </div>
            <div className="canvas__header-actions">
              {stale ? <Badge tone="warn">no data from the daemon</Badge> : null}
              {preview ? <Badge tone="warn">Preview data</Badge> : null}
            </div>
          </div>

          <div className="canvas__scroll">
            <div className="canvas__inner">
              {fleet.error ? (
                <ErrorBanner
                  error={fleet.error}
                  onDismiss={fleet.clearError}
                  onRetry={() => void fleet.refresh()}
                  retrying={fleet.refreshing}
                />
              ) : null}

              {path === "hosts" ? (
                <DesktopOnlyPage surface="hosts" onBack={() => navigate("fleet")} />
              ) : path === "engine" ? (
                <DesktopOnlyPage surface="engine" onBack={() => navigate("fleet")} />
              ) : path === "gates" ? (
                <DesktopOnlyPage surface="gates" onBack={() => navigate("fleet")} />
              ) : fleet.loading && !snapshot ? (
                <Panel title="Reading the daemon" aria-busy="true">
                  <p>Reading health, the board, jobs, sessions, and the open inbox…</p>
                </Panel>
              ) : !snapshot ? (
                <EmptyState
                  icon="◇"
                  title="No snapshot yet"
                  body="The daemon did not answer. The Web build never falls back to mock data."
                  actionLabel="Retry"
                  onAction={() => void fleet.refresh()}
                />
              ) : (
                <div className="view-stack">
                  {stale ? (
                    <div className="notice notice--warn" role="status" data-testid="stale-banner">
                      <Badge tone="warn">stale</Badge> no data from the daemon. Last received{" "}
                      {formatClock(snapshot.fetched_at)};{" "}
                      {fleet.status?.next_retry_at
                        ? "the local retry countdown is in the status strip."
                        : "no local retry countdown is scheduled in this state."}
                    </div>
                  ) : null}
                  {fleet.status?.polling_only ||
                  fleet.status?.stream_state === "reconnecting" ? (
                    <div className="notice notice--warn" role="status">
                      The board stream is unavailable; the 5 second polling fallback is active.
                      Data is still read from the daemon, not from a cache or mock.
                    </div>
                  ) : null}

                  <Panel title="Daemon" data-testid="daemon-panel">
                    <dl className="web-facts">
                      <div>
                        <dt>Node</dt>
                        <dd className="mono">{snapshot.daemon.node}</dd>
                      </div>
                      <div>
                        <dt>Project</dt>
                        <dd className="mono">{snapshot.daemon.project}</dd>
                      </div>
                      <div>
                        <dt>Daemon version</dt>
                        <dd className="mono">{snapshot.daemon.version}</dd>
                      </div>
                      <div>
                        <dt>Origin</dt>
                        <dd className="mono">{snapshot.daemon.origin}</dd>
                      </div>
                      <div>
                        <dt>Source</dt>
                        <dd>{snapshot.source === "daemon" ? "live from daemon" : "preview data"}</dd>
                      </div>
                      <div>
                        <dt>Received</dt>
                        <dd>{formatClock(snapshot.fetched_at)}</dd>
                      </div>
                      <div>
                        <dt>Snapshot changed at</dt>
                        <dd>{formatClock(snapshot.generated_at)}</dd>
                      </div>
                      <div>
                        <dt>Daemon RTT</dt>
                        <dd>{latencyLabel(snapshot.latency.last_ms)}</dd>
                      </div>
                      <div>
                        <dt>Last heartbeat</dt>
                        <dd>
                          {snapshot.daemon.heartbeat_at
                            ? `${formatAge(
                                Math.max(
                                  0,
                                  Math.floor(
                                    (Date.parse(snapshot.daemon.received_at) -
                                      Date.parse(snapshot.daemon.heartbeat_at)) /
                                      1000,
                                  ),
                                ),
                              )} before this health read`
                            : "No supervisor heartbeat is present in /api/v1/health."}
                        </dd>
                      </div>
                    </dl>
                  </Panel>

                  <FleetStructure snapshot={snapshot} stale={stale} />
                  <RunningWorkers
                    snapshot={snapshot}
                    onOpenJob={(job) => setSelectedJobKey(`${job.project}:${job.job_id}`)}
                  />
                  <Lanes
                    snapshot={snapshot}
                    onOpenJob={(job) => setSelectedJobKey(`${job.project}:${job.job_id}`)}
                  />
                  <UnavailableDetails snapshot={snapshot} />
                </div>
              )}
            </div>
          </div>
        </main>

        <aside className="rail rail--right" aria-label="Capacity and storage" tabIndex={0}>
          {snapshot ? (
            <div className="view-stack">
              <CapacityPanel snapshot={snapshot} />
              <SupervisorCard snapshot={snapshot} />
            </div>
          ) : null}
        </aside>
      </div>

      <WebStatusStrip
        snapshot={snapshot}
        connection={fleet.connection}
        status={fleet.status}
      />


      <nav className="web-tabbar" aria-label="Mobile tabs" data-testid="web-tabbar">
        <button
          type="button"
          className="web-tabbar__item"
          data-testid="tab-fleet"
          aria-current={path === "fleet" ? "page" : undefined}
          onClick={() => {
            if (path !== "fleet") navigate("fleet");
            document.getElementById("main-canvas")?.scrollTo?.({ top: 0 });
            document.querySelector(".canvas__scroll")?.scrollTo?.({ top: 0 });
          }}
        >
          <span aria-hidden="true">◈</span>
          Fleet
        </button>
        <button
          type="button"
          className="web-tabbar__item"
          data-testid="tab-workers"
          onClick={() => {
            if (path !== "fleet") navigate("fleet");
            window.setTimeout(
              () => document.querySelector('[data-testid="running-workers"]')?.scrollIntoView?.({ block: "start" }),
              0,
            );
          }}
        >
          <span aria-hidden="true">⚙</span>
          Workers
        </button>
        <button
          type="button"
          className="web-tabbar__item"
          data-testid="tab-inbox"
          onClick={() => setInboxOpen(true)}
        >
          <span aria-hidden="true">◔</span>
          Inbox{inboxCount > 0 ? ` (${inboxCount})` : ""}
        </button>
        <button
          type="button"
          className="web-tabbar__item"
          data-testid="tab-capacity"
          onClick={() => setCapacityOpen(true)}
        >
          <span aria-hidden="true">▥</span>
          Capacity
        </button>
      </nav>

      <WebInboxDrawer
        open={inboxOpen}
        onClose={() => setInboxOpen(false)}
        bridge={bridge}
        snapshot={snapshot}
      />
      <JobDrawer
        job={selectedJob}
        bridge={bridge}
        stale={stale}
        onClose={() => setSelectedJobKey(null)}
      />

      {capacityOpen ? (
        <Drawer open title="Capacity & VPS Storage" onClose={() => setCapacityOpen(false)}>
          {snapshot ? (
            <div className="view-stack">
              <CapacityPanel snapshot={snapshot} />
              <SupervisorCard snapshot={snapshot} />
            </div>
          ) : (
            <EmptyState
              icon="▥"
              title="Capacity unavailable"
              body="There is no board snapshot yet; the Web build shows no zeroes or guessed target."
            />
          )}
        </Drawer>
      ) : null}
    </div>
  );
}
