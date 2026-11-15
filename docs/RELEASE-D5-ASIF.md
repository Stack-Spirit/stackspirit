# StackSpirit D5 ASIF — release and closeout note

**Release baseline:** `main` at `5cb660beb01af85583932d2d65014c10bcf4abbe` (2026-11-15).
This note covers D5a core, D5b bridge/Tauri commands, and D5c Sessions UI.

**Release status:** D5a–D5c are merged and worker-verified on this baseline. The desktop build
enables `features.asif_actions`; the runtime capability probe still decides export and import
independently. **No live SSH/desktop export or import has been executed by the owner or in this
job.** A live VOWP CLI round trip was exercised locally with disposable temp homes; see §1.5
and §5. D6 remains gated on VOWP Phase 2/3.

## 1. Scope and evidence

### 1.1 Shipped surfaces

| Surface | Scope in this release | Commit(s) |
|---|---|---|
| **D5a `spirit-core`** | ASIF 0.4 export-summary and import-response wire types, the private remote-file helper (0700 temp directory, size caps, byte-count check, cleanup), `VowpClient`/`SpiritCore` export and import, and exit-code mapping. | `99270901e9936c4d857c6389f8545b349163f75b` |
| **D5b bridge and Tauri commands** | `SpiritBridge.sessions`, `exportAsif`/`importAsif`, the `asif_export`/`asif_import` commands, native save/open dialogs, local path validation, safe DTOs, MockBridge ASIF scenarios, and the `d5` feature flag. | `ed11b98208595ad4d8c01f0ce345a739425f5713` |
| **D5c Sessions UI** | Sessions drawer, export/import flows, strict/allow-loss controls, loss/warning/unsupported display, partial-ready gating, and engine gate-card updates. | `ebd72e6b24501eadbd4eca5831642803d17fd86c`, `22e093f9b8d1d99eb946f2ba537d8b6ae884894b`, `0a13df8b7d9653d5a2576229a113769c2873de9a`, `5cb660beb01af85583932d2d65014c10bcf4abbe` |

The D5 commit range is `9927090` through `5cb660b` (6 commits). The preceding Desktop MVP
release remains at `f1348e3`; its D5-gated limitation is superseded by this note.

### 1.2 Versions

| Item | Value | Source |
|---|---|---|
| Release baseline | `5cb660beb01af85583932d2d65014c10bcf4abbe` | `git rev-parse HEAD` |
| `spirit-core` crate | `0.1.0`; workspace MSRV `1.85` | `crates/spirit-core/Cargo.toml`, root `Cargo.toml` |
| Tauri Rust crate | `2.12.5` | `Cargo.lock` |
| `tauri-plugin-dialog` | `2.8.4` | `Cargo.lock` |
| `@tauri-apps/api` / `@tauri-apps/cli` | `2.12.4` / `2.12.4` | `package-lock.json` |
| Pinned build container | Rust `1.90.0`, Node `22.23.5` | `docker/desktop-build.Dockerfile` |
| Host re-run toolchain | `cargo 1.99.1`, `node 24.21.0`, `npm 11.19.0` | Commands in §3 |
| VOWP CLI | `vowp 0.3.0` | `vowp --version` |
| ASIF / adapter | ASIF `0.4` (review draft), `vowp-asif` `0.1.1`, ASIF pin `652d9e23ddbe5656b9ceea311df7e09e93c1cd2f` | `docs/VOWP-CONTRACT.md` §11.2.1; live export report |

`vowp 0.3.0` is the CLI version, not the ASIF specification version. ASIF 0.4 is a review
draft. This release does not claim ASIF conformance, a lossless translation, exact request
reconstruction, portable continuation, or native-harness import.

### 1.3 Evidence provenance

The D5 job reports were expected at `.vowp/jobs/<job>/report.md` in the supervisor workspace.
Only one was available:

| Job | Report status | Evidence used here |
|---|---|---|
| `ss-13-asif-core` | **Available**, `status: done`, commit `9927090`. | Exact core commands/results quoted in §1.5 and §3. |
| `ss-14-asif-bridge` | **Not available.** The job directory contains only `brief.md`; the worktree was cleaned. | No report result is quoted. The merged code is in the baseline, and the desktop/UI bridge gates were re-run in this job. |
| `ss-15-asif-ui` | **Not available.** The job directory contains only `brief.md`; the worktree was cleaned. | No report result is quoted. The merged code is in the baseline, and the UI/e2e/axe gates were re-run in this job. |

Every **worker-verified** row in §3 is a command re-run on this baseline in this job, unless the
row says it comes from the `ss-13` report. Missing reports are never marked “passed”.

### 1.4 Capability probe versus a live package transfer

The read-only capability probe was re-run on this baseline:

| Probe | Exact command | Result | What it proves |
|---|---|---|---|
| Version | `vowp --version` | Exit `0`; `vowp 0.3.0`. | The CLI runs. A version string alone does not enable an action. |
| Export capability | `vowp --project stackspirit session export 00000000000000000000000000 --format asif --out -` (stdin `/dev/null`) | Exit `3`; stdout `0` bytes; stderr names the nil session and suggests `vowp session ls`. | The `--format asif` flag parsed and the session was simply not found, which is the contract's “export supported” result. **No package was written.** |
| Import capability | `vowp --project stackspirit session import --help` | Exit `0`; help lists `--format <FORMAT>` and the `asif` value (“ASIF 0.4 (review draft) package. Semantic interchange with declared losses”). | The import help advertises the `asif` format. This is a weaker help-text probe, not an import. |

The desktop does not use these local commands. `AsifGate` runs the same export probe over SSH
against the first active project in the last snapshot, then the import help probe. Exit `2`
with a clap “invalid value”/“possible values” message means export is unsupported; any other
exit is ambiguous and stays disabled. Import is never inferred from export alone.

**What the probe does not prove:** that a real package can be exported or imported on the
connected host, that the remote temp helper works over SSH, or that the desktop dialog flow
works. Those remain separately verified or pending (see §1.5 and §3).

### 1.5 Live package evidence

A **live VOWP CLI** round trip was executed in this job with disposable `HOME` and `VOWP_HOME`
directories and a synthetic idle session. It used the installed `vowp 0.3.0` and deleted every
temp directory afterwards. It did not use a real host, the owner's home, a provider key, or one
of the owner's sessions.

| Step | Exact command shape | Result |
|---|---|---|
| Create source project/session | `vowp init --name asif-smoke-source --no-framework`; `vowp session new --title "ASIF smoke" --json` | Exit `0`; one `idle` session. |
| Export | `vowp --json session export <id> --format asif --out <temp>.asif.zip --report <temp>/report.json` | Exit `0`; `format: "asif"`, `6018` bytes, adapter `0.1.1`, ASIF `0.4`, `loss_count: 5`, losses `no-causal-edges`, `cwd-omitted`, `contexts-reconstructed`, `configuration-unknown`, `vowp-operations-not-captured`; unsupported `vowp.gates`, `vowp.jobs`, `vowp.leases`, `vowp.project-db`. |
| Import | `vowp --json session import <temp>.asif.zip --format asif` into a second disposable home | Exit `0`; new session `state: "idle"`, `role: "user"`, `loss_count: 7` (including `session-id-changed`, `capture-id-not-preserved`, `cwd-rebound`), one reindex warning that the source project was left null, unsupported `asif.checkpoints`, `asif.configurations`, `asif.contexts`. |

The `ss-13-asif-core` report records an equivalent temp-home round trip: export exit `0` with
`6016` bytes and five losses; import exit `0` with an idle session and seven losses; re-importing
the same package into the same home exit `4` with `EVENT_ID_DUPLICATE`. That duplicate-import
result is the conflict path, not a retry.

**This is not a remote or desktop smoke.** It exercises the public VOWP CLI and package format
only. The desktop's SSH remote-file path, native dialogs, and Tauri bridge are covered by fake
runners, unit tests, or MockBridge e2e, not by a live host. The StackSpirit Engine sidecar is not
involved in ASIF, so there is no real-engine call in this release.

## 2. What works

### 2.1 Sessions drawer

- The drawer is opened from the canvas header and lists sessions from the **last snapshot**, not
  from a new remote read. Each row shows id, title, project, state, executor, and updated time.
- The state label maps VOWP's `idle`, `running`, `blocked`, `switching`, and `closed`; an unknown
  state stays visible as `Unknown (<raw>)` instead of being mapped to an invented happy state.
- Empty, disconnected, stale, and missing-snapshot states are explicit. The list is read-only;
  it never creates or starts a session.

Source: `packages/spirit-ui/src/views/sessions/SessionsDrawer.tsx`,
`packages/spirit-ui/src/domain/session.ts`, `packages/spirit-ui/src/state/fleetStore.ts`.

### 2.2 Export

- **Export .asif.zip** is available only when the connection is live, the snapshot is not stale,
  `features.asif_actions` is true, and `AsifGate` proved export.
- The Tauri command opens the native save dialog (or validates a supplied local path), then
  asks `spirit-core` to run
  `vowp --project <project> --json session export <id> --format asif --out <private-remote-file>`.
- The core creates a private remote temp directory with `mktemp -d` and `chmod 700`, checks the
  summary and `stat` size against the 272 MiB archive cap, reads the package with a hard cap,
  verifies the byte count against the summary, and removes the remote directory on success and
  on every post-validation failure path.
- The renderer sees only the local file name, byte count, session id, loss count, losses, and
  unsupported operations. The remote path and package bytes never cross the bridge.
- A cancel returns `null` and writes nothing. A local save is mode `0600` on Unix.

Source: `apps/spirit-desktop/src-tauri/src/commands/asif.rs`,
`crates/spirit-core/src/vowp/asif.rs`, `crates/spirit-core/src/ssh/remote_file.rs`,
`packages/spirit-ui/src/views/sessions/ExportPanel.tsx`.

### 2.3 Import

- **Import .asif.zip** is available only when the same live/gate conditions hold for import.
- The native open dialog chooses a local `.asif.zip`; the host validates the file name, rejects
  symlinks and non-`.zip` names, checks the size before reading, re-checks after reading, and
  refuses a file that changed size.
- The package is streamed to
  `vowp --project <project> --json session import - --format asif`; the response is parsed as
  the normal session shape and **must be `idle`**.
- The result shows the new session id, `idle` state, declared losses, reindex warnings, and
  unsupported operations. The uploaded file name and staging path are never echoed.
- The imported session is a new idle session. StackSpirit does not run a turn, write a capsule,
  create a lease, or dispatch a job after import, and the panel has no run/start/continue/resume
  action.

Source: `apps/spirit-desktop/src-tauri/src/commands/asif.rs`,
`crates/spirit-core/src/vowp/asif.rs`,
`packages/spirit-ui/src/views/sessions/ImportPanel.tsx`.

### 2.4 Strict, allow-loss, and honesty

- The only loss policies are **allow declared losses** (default) and **strict**; they are
  mutually exclusive by construction (`AsifLossPolicy` in core and `strict` xor `allowLoss` in
  the bridge). Passing both is a local `invalid_input` error.
- `--strict` refuses `Blocking`/`Integrity` losses before the package or session becomes
  visible. Inherent omissions and normalizations stay non-fatal, as VOWP defines them.
- `--allow-loss` is passed through; it never makes unsupported semantics safe.
- `--allow-unsupported` has no field, no UI control, and no argv path. Unknown option names are
  rejected. There is no hidden “force” switch.
- A strict refusal (exit `5`) shows a fixed “refused” message and a separate **Choose allow-loss**
  user choice. It is not an automatic retry.
- Every export shows the complete declared-loss list and unsupported operations. Every import
  shows the same plus reindex warnings. A `loss_count` that disagrees with the list is shown as
  a mismatch; neither number is hidden. No result is called lossless or an exact copy.

Source: `packages/spirit-ui/src/domain/session.ts`,
`packages/spirit-ui/src/views/sessions/LossPolicyFields.tsx`,
`packages/spirit-ui/src/views/sessions/AsifReport.tsx`, core `AsifLossPolicy`.

### 2.5 Partial-ready gating

`AsifGate` proves export and import independently. The UI must not show a fully green card when
only one direction is proven:

| Gate state | Export | Import | Display |
|---|---|---|---|
| `Ready { export: true, import: true }` | Enabled | Enabled | `Ready` |
| `Ready { export: true, import: false }` | Enabled | Disabled with “ASIF import capability is not proven.” | `Ready · export only` |
| `Ready { export: false, import: true }` (defensive UI state; the probe reports `Incompatible` when import is advertised without export) | Disabled with “ASIF export capability is not proven.” | Enabled | `Ready · import only` |
| `WaitingForVowpAsif` | Disabled | Disabled | `Waiting` plus the exact `missing` milestones |
| `Incompatible` | Disabled | Disabled | `Incompatible` with detected/required values |
| `Unreachable` | Disabled | Disabled | `Unreachable` with a safe detail |
| Feature flag off | Disabled | Disabled | “not implemented in this build” |

A gate snapshot is scoped to the current host connection. A probe from before the current
connection, a stale snapshot, or a disconnected/stale connection cannot leave an action enabled.
The backend re-checks the gate and build flag on every `asif_export`/`asif_import`, so a stale
renderer cannot bypass it.

Source: `crates/spirit-core/src/engine/gate.rs`,
`packages/spirit-ui/src/domain/session.ts`,
`packages/spirit-ui/src/views/engine/GateCard.tsx`,
`packages/spirit-ui/src/views/engine/GateDrawer.tsx`,
`apps/spirit-desktop/src-tauri/src/commands/asif.rs`.

### 2.6 Security boundaries

- No remote path, remote `cwd`, staging filename, or package bytes reach the renderer. The export
  summary type omits the CLI's `path` and `report_path`; the import session reference omits
  `cwd`; `AsifPackage` has no `Serialize` implementation; typed errors scrub known remote paths.
- The remote-file helper is crate-private and not part of the read contract. Every dynamic
  argument is a separate, POSIX-quoted argv element; no shell pipeline, `;`, `&&`, or
  redirection is used.
- Remote temp directories are `0700` and removed on success and on every post-validation
  failure path; VOWP writes private package/report files as `0600`. A cleanup failure is
  reported and no package is returned. A temp path the process did not create is rejected
  before use.
- There is no token or provider key in the ASIF bridge, DTOs, dialog permissions, or logs. The
  Tauri capability adds `dialog:default` only; no shell permission is added.
- The Web bridge has no `SpiritBridge.sessions` group and no ASIF method. D5 is desktop-only, so
  this release introduces no browser token or browser-side provider key.
- An exported package may contain secrets. VOWP scrubs host paths by default but does not detect
  exact secret values; the CLI's `--redact-file` exists for a specific list and is not exposed in
  the UI. Do not treat an export as secret-free.

## 3. Verification

### 3.1 Gate results on `5cb660b`

| Check | Exact command | Result | Status |
|---|---|---|---|
| Core format | `cargo fmt --all -- --check` | Exit `0`; no output. | **worker-verified** (re-run) |
| Core ASIF tests | `cargo test -p spirit-core --features fixtures asif` | `34 passed; 0 failed`: `1` unit + `28` `tests/asif.rs` + `5` ASIF gate tests. | **worker-verified** (re-run; matches `ss-13`) |
| Core full suite | `cargo test -p spirit-core --features fixtures` | `196 passed; 0 failed` across 13 suites (including doc-tests). | **worker-verified** (re-run; matches `ss-13`) |
| Core clippy | `cargo clippy -p spirit-core --all-targets -- -D warnings` | Exit `0`; finished with no warnings. | **worker-verified** (re-run) |
| CLI regression | `cargo test -p spirit-cli` | `27 passed; 0 failed`. | **worker-verified** (re-run) |
| Remote-file size / `0700` / cleanup | Included in `cargo test -p spirit-core --features fixtures asif`: `asif_export_uses_the_documented_two_step_argv`, `asif_export_enforces_the_archive_cap_before_the_read`, `asif_export_verifies_the_package_byte_count`, `asif_export_removes_the_temp_dir_on_every_failure_path`, `asif_export_reports_cleanup_failure_and_returns_no_package`, `asif_export_rejects_a_temp_path_it_did_not_create`, `asif_export_scrubs_the_remote_path_from_vowp_errors`. | `28 passed; 0 failed` in `tests/asif.rs`. Uses `FakeCommandRunner`; no real SSH. | **worker-verified** for the helper logic; **owner-run/pending** for a live remote transfer |
| Shared UI unit tests | `npm test -w @stackspirit/ui` | `226 passed; 0 failed` in 21 files. | **worker-verified** (re-run) |
| D5-specific UI/bridge suites | `npx vitest run src/test/sessions src/test/engine/gates.test.tsx src/bridge/asif.test.ts` | `61 passed; 0 failed` in 5 files. | **worker-verified** (re-run) |
| Desktop renderer boundary | `npm test -w @stackspirit/desktop` | `1 passed; 0 failed` (the `@tauri-apps/api` import-boundary test). | **worker-verified** (re-run) |
| Web regression | `npm test -w @stackspirit/web` | `73 passed; 0 failed` in 4 files. | **worker-verified** (re-run) |
| Desktop Tauri host | `scripts/desktop-build-container.sh check` | Exit `0`; all desktop gates passed, including `cargo test -p spirit-desktop`: `9 passed; 0 failed` ASIF command tests. | **worker-verified** (re-run; pinned Rust 1.90 / Node 22.23.5 container) |
| Typecheck | `npm run typecheck` | Exit `0` for UI, desktop, and Web. | **worker-verified** (re-run) |
| Lint | `npm run lint` | Exit `0`; `eslint --max-warnings 0` for all three workspaces. | **worker-verified** (re-run) |
| Build | `npm run build` | Exit `0`; UI typecheck, desktop and Web `tsc` + Vite builds completed. | **worker-verified** (re-run) |
| D5 Playwright/axe | `npm run test:e2e -w @stackspirit/desktop -- --timeout=90000 sessions-asif` | `1 passed; 0 failed`; three `expectNoWcagViolations` checks in the flow. | **worker-verified** (MockBridge-only) |
| Full e2e | `npm run test:e2e` | `15 passed; 0 failed`: desktop `5` and Web `10`. | **worker-verified** (MockBridge-only) |
| Capability probe | The three commands in §1.4. | Export exit `3`, stdout empty; import help exit `0` and lists `asif`; version `0.3.0`. | **worker-verified** (re-run) |
| Live CLI package round trip | The temp-home commands in §1.5. | Export exit `0` / `6018` bytes / `5` losses; import exit `0` / `idle` / `7` losses / `1` warning. | **worker-verified** (local temp homes, not SSH/desktop) |
| Live SSH remote export/import | The desktop's actual `mktemp`/`chmod`/`stat`/`cat`/`rm` path against a real host. | Not executed. | **owner-run/pending** |
| Native desktop dialogs and GUI | Packaged app, real save/open dialogs, Tauri bridge against a real VOWP home. | Not executed. | **owner-run/pending** |
| macOS/Windows packaging, signing, keychain | `docs/OWNER-VERIFICATION.md` build/keychain steps. | Not executed for D5. | **owner-run/pending** |

The `ss-13` report also records `cargo fmt`, the `34` ASIF tests, the `196` full tests,
`cargo clippy`, and `cargo check -p spirit-cli` as passing at commit `9927090`. Its live probe
used temp homes and is quoted in §1.5. The `ss-14` and `ss-15` reports are **not available**
(§1.3); their merged code is covered by the re-runs above.

### 3.2 What the automated tests do and do not prove

- The core ASIF tests use `FakeCommandRunner` and golden fixtures. They prove argv construction,
  quote/validation rules, `0700`, the caps, byte-count checks, cleanup on success and failure,
  path scrubbing, loss/warning/unsupported parsing, strict/allow-loss exclusivity, exit mapping,
  and idle-only imports. They do not prove a real SSH host or a real `vowp` binary.
- The bridge tests use `MockBridge` and typed DTOs. They prove partial-ready gating, safe payloads,
  strict refusal, conflict behavior, no auto-run, and no force/remote-cwd option. They do not
  prove the native dialog or a live Tauri invoke.
- The desktop command tests validate local paths, symlink refusal, DTO scrubbing, idle imports,
  gate refusals, and the `d5`/D6 flag split. They do not open a native dialog.
- The Playwright D5 spec drives the preview `MockBridge`, not a live host. It includes three axe
  checks on the Sessions drawer, export result, and import result. The spec rewrites two
  preview pseudo-ULIDs to valid 26-character ULIDs at the network boundary; the shipped fixture
  is unchanged.
- The automated axe checks complement, but do not replace, an owner GUI/assistive-technology
  pass.

## 4. Security and limits

| Limit | Value | Who enforces it |
|---|---|---|
| ASIF archive | `272 MiB` (`285,212,672` bytes) | Desktop local file check and remote summary/`stat` check before any read; VOWP/adapter also enforces it. |
| ASIF summary / import JSON | `16 MiB` | Every summary and import response is read with `ASIF_JSON_CAP_BYTES`. |
| Single resource | `64 MiB` | VOWP/adapter; a violation is a typed error, never truncation. |
| Total resources | `256 MiB` | VOWP/adapter; a violation is a typed error, never truncation. |
| Package members | `1,024` | VOWP/adapter. |
| Portable path | `4,096` characters | VOWP/adapter; the desktop also caps remote paths and local selectors. |

The desktop refuses an oversized local package **before** reading it and an oversized remote file
**before** `cat`. A `stat`/`cat` size disagreement is a retryable transport error and no package
is returned. A local file that changes size while being read is a conflict. Nothing is truncated
or partially imported.

Exit mapping (`VOWP-CONTRACT.md` §11.2.4):

| VOWP exit | Condition | StackSpirit result | Retry/mutation |
|---|---|---|---|
| `2` | Invalid package/JSON/schema/version/resource, unsafe path, torn source log, bad flag, oversized stdin | `invalid_input` or `parse`; VOWP's safe message is shown. | No import; no retry mutation. |
| `3` | Unknown source session or remote node | `not_found`; “Unknown source session (exit 3)”. | Manual retry only after the source is corrected. |
| `4` | Target exists, or an imported event id belongs to another session | `conflict`; “Import conflict (exit 4)”. | Non-retryable. StackSpirit never retries with a changed package. |
| `5` | Strict refusal, required unsupported ASIF feature, other adapter refusal | `refused`; “Refused (exit 5)”. | No force switch, no `--allow-unsupported`, no auto-retry. |
| `7` | Remote transport, truncated/corrupt export stream | `remote`; “Transport failure (exit 7)”. | Manual retry; no package is kept or mutated. |

Other notes:

- `--strict` and `--allow-loss` are mutually exclusive. The core enum makes both flags
  impossible at once, and the Tauri input rejects both. `--allow-unsupported` has no variant or
  field and is rejected as an unknown option.
- Required unsupported features are refusals (exit `5`), not warnings that a force switch could
  override.
- The remote helper's errors never carry the remote temp path or package path; a failed cleanup
  is reported with no package returned.
- The package goes only to the chosen VOWP home. StackSpirit does not send it to any other
  server, index it, or log its bytes. The import response does not echo the uploaded file name
  or staging path.
- Exported packages may contain secrets. The UI does not claim they are secret-free; the CLI's
  `--redact-file` is the documented way to remove a known literal.

## 5. Owner/live smoke status

**No live SSH or desktop export/import has been executed by the owner or in this job.** The
only live package exercise in this job is the local CLI round trip in §1.5; the `ss-13` report
records an equivalent temp-home round trip. The desktop flow is still verified only by unit
tests, the Tauri host tests, and MockBridge e2e.

### 5.1 Exact owner-run CLI smoke (safe, disposable)

Run this on a machine with `vowp 0.3.0` (or the release that provides ASIF) and `jq`. It uses a
throwaway `HOME`, two throwaway VOWP homes, and synthetic sessions; it never reads the owner's
home or a real key. Source and target are separate because VOWP event ids are global per home.

```sh
SMOKE="$(mktemp -d /tmp/stackspirit-asif-smoke.XXXXXX)"
mkdir -p "$SMOKE/source/home" "$SMOKE/source/vowp-home" "$SMOKE/source/project" \
         "$SMOKE/target/home" "$SMOKE/target/vowp-home" "$SMOKE/target/project"
git -C "$SMOKE/source/project" init -q
git -C "$SMOKE/target/project" init -q

(cd "$SMOKE/source/project" && \
  HOME="$SMOKE/source/home" VOWP_HOME="$SMOKE/source/vowp-home" \
  vowp init --name asif-smoke-source --no-framework)
(cd "$SMOKE/target/project" && \
  HOME="$SMOKE/target/home" VOWP_HOME="$SMOKE/target/vowp-home" \
  vowp init --name asif-smoke-target --no-framework)

SID="$(cd "$SMOKE/source/project" && \
  HOME="$SMOKE/source/home" VOWP_HOME="$SMOKE/source/vowp-home" \
  vowp session new --title "ASIF smoke" --json | jq -r .id)"

(cd "$SMOKE/source/project" && \
  HOME="$SMOKE/source/home" VOWP_HOME="$SMOKE/source/vowp-home" \
  vowp --json session export "$SID" --format asif \
    --out "$SMOKE/session.asif.zip" --report "$SMOKE/report.json")

(cd "$SMOKE/target/project" && \
  HOME="$SMOKE/target/home" VOWP_HOME="$SMOKE/target/vowp-home" \
  vowp --json session import "$SMOKE/session.asif.zip" --format asif)
```

Expected observations:

1. Export exits `0`; the summary has `format: "asif"`, a non-zero `bytes`, `adapter_version`
   `0.1.1`, `asif_version` `0.4`, and a declared-loss list. The package is not described as
   lossless.
2. Import exits `0`; `session.state` is `idle`; `report.loss_count` is greater than zero; the
   loss list includes `session-id-changed`, `capture-id-not-preserved`, and `cwd-rebound`; and
   `warnings`/`unsupported` are shown rather than hidden.
3. Re-importing the same package into the target home exits `4` and names the duplicate event
   id. Do not retry it with a mutated package.
4. Delete the whole smoke tree, including both homes and the package:
   `rm -rf "$SMOKE"`. Do not paste the raw export summary into an issue or report without
   removing its local `path`/`report_path` fields.

### 5.2 Owner-run desktop smoke (not executed)

To prove the desktop path rather than the CLI, use a disposable VOWP host and a throwaway SSH
key generated only for the test (never the owner's real key or home):

1. Prepare a disposable VOWP home/project on a test host using the commands in §5.1, plus a
   throwaway SSH key and SSH access for the test user.
2. In the packaged D5 app, add the host with its confirmed fingerprint, connect, and confirm the
   ASIF gate proves the directions you intend to use. A waiting or partial gate must leave the
   missing action disabled with the visible reason.
3. Open **Sessions**, export the synthetic session, and verify the local file name, byte count,
   declared losses, and unsupported operations. The remote path must never appear.
4. Import the package back into the disposable target home, and verify the new session id,
   `idle` state, losses, warnings, and unsupported operations. Confirm there is no run/continue
   action and that VOWP did not start a turn.
5. Test one conflict by re-importing the same package. Confirm it does not retry automatically,
   mutate the package, or offer a force/`--allow-unsupported` switch. A strict refusal can be
   exercised with a package that has a blocking/integrity loss; if none is available, record
   that the strict path was not exercised rather than marking it passed.
6. Disconnect, delete the disposable home, project, package, and throwaway key. Record only
   redacted command output, never a host, key, token, or package.

**Status: pending.** No owner evidence exists for any step above. This note does not imply that
the owner ran the CLI or desktop smoke.

## 6. README refresh

`README.md` now describes `.asif.zip` sessions as shipped in D5, not gated:

- The capability-gates section explains per-direction probing, the `d5` feature flag, and the
  partial-ready display.
- The cross-project section lists D5 desktop ASIF as available now and keeps D6 local
  orchestrators explicitly gated on VOWP Phase 2 (`v4-local-2e/2f/2g`) and Phase 3
  (`v4-local-3*`).
- The documentation list links this note; the note names the `ss-13`, `ss-14`, and `ss-15` D5
  reports and their evidence status.
- The README does not claim that the owner smoke ran.

## 7. Open follow-ups

1. **D6 local orchestrators remain gated.** Required VOWP milestones are Phase 2
   (`v4-local-2e/2f/2g`) and Phase 3 (`v4-local-3*`: local opencode app host, bridge, MCP proxy,
   orchestrator materialization, backfill, local cache). `features.local_orchestrator_actions`
   stays false; a Ready gate still shows “not implemented in this build”.
2. **Owner live smoke is pending.** Execute §5.1 and §5.2 on a disposable host/home, with a
   throwaway key, and record redacted evidence. No real host or home.
3. **Remote transfer is not live-tested in CI.** The `0700` temp directory, caps, byte-count
   check, and cleanup are exercised with `FakeCommandRunner`. A live SSH host is the remaining
   proof for that helper.
4. **Native dialogs are not automated.** The Tauri command tests validate paths and DTOs, but
   the save/open dialogs need the packaged app.
5. **ASIF limitations are unchanged.** ASIF 0.4 is a review draft; no conformance, lossless
   translation, exact request reconstruction, portable continuation, or native-harness import is
   claimed. `--allow-unsupported` and force switches are deliberately absent. Packages may
   contain secrets; VOWP does not detect exact secret values.
6. **VOWP-side follow-ups.** The independent `v4-asif-verify` job and a published adapter remain
   VOWP-side work; the optional `v4-asif-web` daemon route is not used by this desktop release.
   The package transfer uses the public CLI over SSH with a private remote file, not `--out -`
   (which is used only by the read-only export capability probe), the hidden `--asif-stream`, or
   the daemon ASIF routes.
7. **Documentation still describes D5 as gated.** `docs/PLAN.md` §9.7 and
   `docs/OWNER-VERIFICATION.md` (§1.2, §3.6, §4.5, §6.3) predate this release. They were outside
   this job's write scope and need a follow-up docs change; this note and `README.md` are the
   current D5 statement.
