/**
 * Pure domain helpers reused from `packages/spirit-ui`.
 *
 * The shared package's public entry exports the shell and design-system primitives but not
 * these job/domain helpers. Copying lane logic into the Web app would create a second
 * parser, so the Web app imports the same source module by relative path. Making these
 * public exports in `packages/spirit-ui/src/index.ts` is a follow-up outside this job's
 * ownership.
 */
export {
  LANES,
  countsFromProjects,
  findJobTitle,
  jobStateChip,
  laneCount,
  laneFooterText,
  laneForState,
  sortWorkerCards,
  type LaneDefinition,
} from "../../../../packages/spirit-ui/src/domain/fleet";
