/**
 * Prop-driven view components reused from `packages/spirit-ui`.
 *
 * `FleetCanvas` and `CapacityRail` read the desktop store and require a complete desktop
 * `FleetView`, so the Web shell cannot mount them without inventing capacity/supervisor
 * data. These smaller components are pure props and render the same cards and lanes.
 */
export { JobCard } from "../../../../packages/spirit-ui/src/views/fleet/JobCard";
export { JobLane } from "../../../../packages/spirit-ui/src/views/fleet/JobLane";
export { WorkerCard } from "../../../../packages/spirit-ui/src/views/fleet/WorkerCard";
