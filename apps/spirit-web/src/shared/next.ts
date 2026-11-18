/**
 * Meadow ("next") components reused from `packages/spirit-ui/src/next`.
 *
 * `@stackspirit/ui` exposes only its root entry and `./styles.css`, so these are
 * deep source imports, the same pattern `shared/views.ts` already uses for the
 * classic job views. Making `./next` a package export is a follow-up outside this
 * job's ownership.
 *
 * Only prop-driven components belong here. `NextShell`, `Sidebar`, `ServerPage`,
 * `WorkersPage`, `JobRows`, `PageHeader`, and `ErrorNotice` read the desktop store
 * (`useFleetStore`/`useShellUi`) and do not fit the read-only Web cockpit.
 */
export { NodeMap, type MapBranch, type MapNode } from "../../../../packages/spirit-ui/src/next/NodeMap";
export { SpiritMark } from "../../../../packages/spirit-ui/src/next/SpiritMark";
