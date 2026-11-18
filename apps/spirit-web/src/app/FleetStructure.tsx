import type { WebFleetView } from "@stackspirit/ui";
import { Panel } from "@stackspirit/ui";

import { LANES } from "../shared/domain";
import { NodeMap, type MapBranch, type MapNode } from "../shared/next";

/**
 * Lane tones for the Meadow node map. The desktop Workers page uses the same
 * running → ok / review → info / attention → warn mapping; Meadow has no separate
 * "verifier" tone, so review/fix shares the calm blue.
 */
const LANE_TONES = {
  running: "ok",
  ready: "neutral",
  review_fix: "info",
  parked_blocked: "warn",
  done: "faint",
} as const;

function jobCountLabel(count: number): string {
  return `${count} job${count === 1 ? "" : "s"} in this snapshot`;
}

/**
 * Read-only structure map: daemon → project → exact lane counts.
 *
 * This reuses the desktop `NodeMap` (a pure props component) without any of the
 * desktop store or job actions. Leaves are labels, not links: the Web cockpit has
 * no second project and nothing to open from a lane summary.
 */
export function FleetStructure({
  snapshot,
  stale,
}: {
  snapshot: WebFleetView;
  stale: boolean;
}) {
  const total = Object.values(snapshot.project.jobs_by_state).reduce(
    (sum, count) => sum + count,
    0,
  );
  const leaves: MapNode[] = LANES.map((lane) => ({
    id: `lane-${lane.key}`,
    label: lane.label,
    sub: `${snapshot.lanes.counts[lane.key] ?? 0} ${lane.footerLabel}`,
    tone: LANE_TONES[lane.key],
  }));
  const branches: MapBranch[] = [
    {
      node: {
        id: `project-${snapshot.project.name}`,
        label: snapshot.project.name,
        sub: jobCountLabel(total),
        tone: "neutral",
      },
      leaves,
    },
  ];

  return (
    <Panel title="Fleet structure" data-testid="web-structure">
      <NodeMap
        label="Daemon, project, and job lanes"
        root={{
          id: "daemon",
          label: snapshot.daemon.node,
          sub: `daemon ${snapshot.daemon.version}`,
          tone: stale ? "warn" : "ok",
        }}
        branches={branches}
      />
      <p className="muted small">
        One daemon, one project. Lane counts are the exact board counts; the map is
        read-only and opens nothing.
      </p>
    </Panel>
  );
}
