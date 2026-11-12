# v0.2 / v0.3 — cross-node session sync

Status: current for the commands shown; **partly superseded by v0.2**. v0.2's remote execution (`docs/design/v0.2/home-and-surfaces.md`
§7.2) keeps one session on the home and moves only the **executor** to another machine. That is the
product behaviour "the same conversation from the laptop and the phone"; a session no longer has to
be copied between nodes for a human to continue it.

This note now records two things:

1. what v0.2 ships instead of a bundle move;
2. what remains a real v0.3 problem for two homes or offline copies.

## What v0.2 ships

- **One home owns the log.** A laptop surface runs `vowp __exec`, streams the harness events over
  SSH into `vowp session ingest` on the home, and the home assigns `seq` and appends. A drop is
  spooled and replayed; ingest is idempotent by event id. The same session id and the same log are
  used on every surface.
- **The session does not fork.** There is no child session, no second log and no import for the
  laptop case. The v0.1 bundle remains for an explicit export/import to another installation.
- **A lease decides who writes.** The old surface is cut off with a notice and follows read-only;
  the new surface continues from the capsule. See the v0.2 spec §7.3.
- **A bundle move still exists** for an installation that is offline or for a deliberate copy to a
  second home: `vowp session export` / `session import` behave exactly as in v0.1 (`vowp-bundle-v1`,
  checksum, child session, capsule + tail, inline blobs ≤ 64 KiB).

So the v0.1/v0.2 answer to "move a session between nodes without hand-copying a file" is: you do
not move it; you move the executor. The bundle is the escape hatch, not the everyday path.

## What remains for v0.3

- **`session push <node> <session>`** as the remaining convenience wrapper around the existing
  bundle and `--node` transport, for two-home or offline use. `session pull <node> <session>` landed
  in the first slice below.
- **Directory copy.** Copy `sessions/<id>/` (log, blobs, capsules, `adapters.json`) with a manifest,
  checksums and a size cap, then run `vowp reindex`; preserve the full log rather than the bundle's
  capsule + tail.
- **Divergence reporting, never a silent merge.** If both sides advanced the same session, report
  the two heads and require an explicit choice.
- **Daemon transport.** The optional second transport after the v0.3 HTTP API exists. SSH stays the
  only v0.2/v0.3 transport until then.

## Non-goals (unchanged)

- A distributed database, leader election or two supervisors writing one project.
- Automatic conflict resolution when both nodes advanced the same session.
- Copying a running job's worktree or uncommitted files. A job stays homed on one node.
- A hosted relay.

## Existing crates it builds on

- `vowp-core::session::bundle` — `vowp-bundle-v1`, checksum, capsule, tail and inline blobs.
- `vowp-core::session::service` — import creates a child session and seeds the first turn.
- `vowp-store::session_log` and `vowp-store::reindex` — the log is authoritative and the index is
  rebuildable.
- `vowp-cli::remote` — `build_ssh_argv`, `strip_node_flag` and the exit-code mapping.
- `vowp-cli::cmd::session` — `remote_export`, `remote_import` and `remote_switch` already stream a
  bundle over SSH.
- The `nodes` table — name, SSH target, capabilities and last-seen time.

## Open questions (v0.3)

1. **Child vs same id.** The v0.1 child rule is still the default. The second slice adds an
   explicit `--same-id` mode: it preserves the origin id only when it is absent locally, refuses a
   duplicate, and treats a `reindex` event-id collision as a conflict. An automatic choice between
   the two modes is still open; divergence needs the two-head report below.
2. **Direction.** Explicit `pull`/`push` first; a bidirectional `session sync` only with a visible
   divergence report. Never guess a merge.
3. **Blobs.** A directory copy needs a size limit, a progress display and a clear "blob
   unavailable" marker for the rest.
4. **Auth and paths.** The remote home, project and cwd may differ. The bundle already carries
   `cwd`; import must keep the existing allow-list check.
5. **Offline queue.** A push that fails should be retryable without duplicating a child; decide
   whether the request is idempotent by content checksum.
6. **Lease interaction.** A session pulled to a second home cannot have two live leases. v0.3 must
   define whether a pull requires the source lease to be released or is a read-only snapshot.

## First v0.3 slice (implemented)

`vowp session pull <node> <session>` calls the remote `session export --out -` over the existing
SSH argv/capture path, stages the bundle in a private temporary directory, verifies its format,
protocol version and checksum with `bundle::from_json`, and imports it as a local child through the
same path as `session import`. It prints the source and child ids (`--json`:
`{"node","source_session","child_session"}`) and removes the staged file on every path. A failed
remote run, malformed/truncated bundle or checksum mismatch imports nothing; a local node is
refused.

The tests use the existing fake-`ssh` harness and a temporary remote home:
`session_pull_imports_a_child_and_prints_both_ids`,
`session_pull_refuses_a_corrupt_bundle_before_importing` and
`session_pull_reports_a_failed_remote_export_and_imports_nothing`. The checksum test fails if
verification is removed, and the success test fails if the child's `parent_session_id` is wrong.

Still out of scope for this slice: directory copy, `session push`, bidirectional sync/merge, daemon
transport, lease interaction and offline queueing. This is independent of v0.2 and does not block
the v0.2 release.

## Second v0.3 slice (implemented)

`vowp session import <file> --same-id` and `vowp session pull <node> <session> --same-id` keep the
origin session id instead of creating a child. The mode is refused with exit 4 when the id already
exists locally as a row or as `sessions/<id>/`; the existing session is never overwritten or
merged. A same-id session has no parent. The importer writes a fresh `session.created`, copies the
whole bundle tail into the log with original event ids and timestamps (sequence numbers are
reassigned contiguously), attaches the bundle capsule, writes inline blobs and runs `reindex`.
A duplicate event id anywhere in the index is a conflict, not a warning. The default child import
and its output are unchanged; `--same-id` pull JSON is
`{"node","source_session","same_id_session"}`.

The tests are `same_id_import_keeps_the_origin_id_without_a_parent`,
`same_id_import_refuses_an_existing_session_and_changes_nothing`,
`same_id_import_rejects_a_duplicate_event_id_before_writing`,
`child_import_still_creates_a_child_with_the_parent` and the CLI tests
`session_pull_same_id_imports_the_origin_id_without_a_parent`,
`session_pull_same_id_refuses_an_existing_session_without_mutating_it`,
`session_pull_same_id_refuses_a_corrupt_bundle_before_importing` and
`session_import_same_id_keeps_the_bundle_id_and_refuses_a_second_import`.

Still out of scope: directory copy, divergence reporting (two heads), merge, bidirectional sync,
`session push`, lease interaction and offline queueing. A same-id pull is still a one-way snapshot;
the source is neither modified nor locked. The bundle carries only the replayable tail, so the
local log is a partial copy even though it keeps the origin id.
