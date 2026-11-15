# StackSpirit Desktop MVP — release and closeout note

**Release baseline:** `main` at `f1348e301fd8749d7fdbec9093ace2ca22b0d253` (2026-11-15).
This note covers the D1–D4 desktop app, the W1 read-only Web shell, and the C1 `spirit` CLI.
The owner-run platform checklist has **not** been executed; see §7.

**Release status:** D1–D4, W1 (read-only), and C1 are merged and worker-verified. The Desktop MVP
is **not owner-verified**: macOS/Windows packaging, the real OS-keychain round trip, and a live
daemon/Web smoke remain pending.

## 1. Scope and versions

### Shipped surfaces

| Surface | Scope in this release | State |
|---|---|---|
| **D1 `spirit-core`** | Host profiles, outbound SSH and host-key pinning, VOWP JSON parsing, view models, engine supervision. | Merged. |
| **D2 desktop shell** | Tauri 2 host, `SpiritBridge`, host manager, connect/disconnect state. | Merged. |
| **D3 fleet canvas** | VPS root, worker ring, running cards, five job lanes, capacity rail, inbox and session metadata. | Merged. |
| **D4 engine onboarding** | Bundled pinned `opencode` sidecar as the StackSpirit Engine, runtime-verified included models, BYOK in the OS keychain, opt-in external harnesses, honest VOWP gate cards. | Merged; the D4 onboarding report is `done`. |
| **W1 read-only Web** | `@stackspirit/web` shell over the VOWP daemon HTTP/SSE API, including the read-only job drawer and review/fix gate data. No writes and no browser token. | Merged. |
| **C1 `spirit` CLI** | Host, fleet, capacity, gates, engine status, JSON output, fixture mode, and typed exit codes over `spirit-core`. | Merged. |

### Versions and commit

| Item | Value | Source |
|---|---|---|
| Release baseline | `f1348e301fd8749d7fdbec9093ace2ca22b0d253` | `git rev-parse HEAD` on `main`. |
| Release commit range | 33 commits from `c6bbd63` through `f1348e3` | `git log --oneline main`. |
| Tauri | `2.12.5` Rust crate; `@tauri-apps/api` and `@tauri-apps/cli` `2.12.4` | `Cargo.lock`, `package-lock.json`. |
| Rust | `1.90.0` in the build container (`rust:1.90-bookworm`); workspace `rust-version` is `1.85` | `docker/desktop-build.Dockerfile`, `Cargo.toml`. |
| Node | `22.23.5` in the build container | `docker/desktop-build.Dockerfile`. |
| StackSpirit Engine | `opencode` `1.19.5` with a pinned archive SHA-256 per target | `engine/manifest.json`. |
| `spirit-core` crate | `0.1.0` | `crates/spirit-core/Cargo.toml` via `[workspace.package]`. |

### Surface commit ranges

| Surface | Commits |
|---|---|
| Plan and contracts | `c6bbd63`, `89333d7` |
| D1 core | `f00ac9b`, `4193cb0`, `71c7aa9`, `f50d752`, `0ad14b6` |
| D2 shell | `987ba41`, `6a16f5e` |
| D3 fleet canvas | `3763da5`, `35225a8`, `ad74dcb` |
| D4 engine onboarding | `662f75c`, `e97ad90`, `a45c97f`, `57d39a2`, `e28f05f`, `9de8325`, `9193af1`, `bf907be` |
| C1 CLI | `858383c` |
| Shared UI extraction | `f4cb6fe` |
| Linux build container | `ec3c53f` |
| Web contract | `7664481` |
| Owner checklist | `ba2307c`, `7cea624`, `28b69cf` |
| W1 Web shell and job detail | `d270cec`, `479dae7`, `420d9d4`, `fa2623f`, `0a7cc72`, `f1348e3` |

## 2. Verified evidence

### Evidence provenance

The dependency job reports named in this job's brief were not present in the workspace when
this note was written; only the D4 onboarding report (`ss-4-engine-onboarding`) was available.
Specifically, the reports for `ss-5-desktop-build-env`, `ss-6-ui-extract`, `ss-7-web-contract`,
`ss-8-cli`, `ss-9-owner-verification`, `ss-10-d4-post-escalation`, `ss-11-web-readonly`, and
`ss-16-web-job-detail` were absent. This note therefore does not quote gate results it could not
read. Every **worker-verified** row below is a command re-run on the release baseline `f1348e3`
on 2026-11-15, with the exact result shown. **Owner-run/manual** rows remain unexecuted and are
marked as such.

### Gate table

| Gate family | Exact command | Result | Status | Evidence |
|---|---|---|---|---|
| Core format | `cargo fmt --all -- --check` | Exit `0`; no output. | **worker-verified** | Re-run at `f1348e3`. |
| Core tests | `cargo test -p spirit-core --features fixtures` | `168 passed; 0 failed` across 11 test binaries plus doc-tests. | **worker-verified** | Re-run at `f1348e3`. |
| Core clippy | `cargo clippy -p spirit-core --all-targets -- -D warnings` | Exit `0`; finished with no warnings. | **worker-verified** | Re-run at `f1348e3`. |
| Desktop npm install | `npm ci` | Exit `0`; workspaces installed from the lockfile. | **worker-verified** | Re-run at `f1348e3`. |
| Desktop typecheck | `npm run typecheck` | Exit `0` for `@stackspirit/ui`, `@stackspirit/desktop`, and `@stackspirit/web`. | **worker-verified** | Re-run at `f1348e3`. |
| Desktop lint | `npm run lint` | Exit `0`; `eslint --max-warnings 0` for all three workspaces. | **worker-verified** | Re-run at `f1348e3`. |
| Desktop unit tests | `npm test` | `247 passed; 0 failed`: shared UI `173` in 17 files, desktop `1` in 1 file, Web `73` in 4 files. | **worker-verified** | Re-run at `f1348e3`. |
| Desktop build | `npm run build` | Exit `0`; UI typecheck, desktop `tsc` + Vite, and Web `tsc` + Vite builds completed. | **worker-verified** | Re-run at `f1348e3`. |
| Desktop and Web e2e | `npm run test:e2e` | `14 passed; 0 failed`: desktop `4` and Web `10`. The Web suite is MockBridge-only. | **worker-verified** | Re-run at `f1348e3`. |
| Docker Tauri host check | `scripts/desktop-build-container.sh check` | Exit `0`; `cargo fmt`, engine fetch, `npm ci`, typecheck, lint, test, build, `cargo check -p spirit-desktop`, and `cargo test -p spirit-desktop` all passed; script ended `all desktop gates passed`. | **worker-verified** | Re-run at `f1348e3`; image `stackspirit-desktop-build:local`. |
| Docker Tauri host build | `scripts/desktop-build-container.sh tauri-build` | Exit `0`; frontend build succeeded and Tauri printed `Built application at: /cache/target/debug/spirit-desktop`; `--no-bundle`, so no `.deb`/AppImage. | **worker-verified** | Re-run at `f1348e3`. |
| Checksum self-test | `bash scripts/verify-engine-checksum.sh --self-test` | `self-test fixture ok`; `self-test ok (a wrong checksum is rejected)`. | **worker-verified** | Re-run at `f1348e3`. |
| Engine fetch | `node scripts/fetch-engine.mjs --dry-run --target x86_64-unknown-linux-gnu` | `already present and verified`; the Docker check downloaded the pinned `opencode` `1.19.5` archive, matched its SHA-256, and wrote only the gitignored sidecar. | **worker-verified** | Re-run at `f1348e3`. |
| CLI tests and build | `cargo test -p spirit-cli`; `cargo build -p spirit-cli` | `27 passed; 0 failed`; build exit `0`. | **worker-verified** | Re-run at `f1348e3`. |
| CLI help and fixture reads | `cargo run -p spirit-cli -- --help`; `cargo run -p spirit-cli -- --fixtures fleet --json`; `--fixtures capacity --json`; `--fixtures gates --json` | Help lists `host`, `fleet`, `capacity`, `gates`, `engine`, `version`; all three fixture reads exit `0` and print JSON plus a `fixture data` banner on stderr. | **worker-verified** | Re-run at `f1348e3`. |
| Shared UI | `npm test -w @stackspirit/ui` | `173 passed; 0 failed` in 17 files; `npm run build -w @stackspirit/ui` exit `0`. | **worker-verified** | Re-run at `f1348e3`; also inside the Docker check. |
| Web bridge | `npm test -w @stackspirit/web`; Web Playwright projects | `73 passed; 0 failed` in 4 files; `10 passed` MockBridge browser tests. No live daemon was contacted. | **worker-verified** for unit and MockBridge e2e; **owner-run/manual pending** for a live `vowp-daemon` smoke | Re-run at `f1348e3`. |
| macOS/Windows app, real keychain, GUI | `docs/OWNER-VERIFICATION.md` §2–§5 | Not run. No owner evidence rows exist. | **owner-run/manual — pending** | Checklist header records “not executed”. |

The core test run includes the security, contract, gate, SSH, and store suites. The desktop
Playwright flow is browser-mode against `MockBridge`; it is a regression check, not proof of a
real SSH host or OS keychain. The Tauri host crate reports 0 Rust unit tests; renderer behavior
is covered by the Vitest and Playwright suites. Host core and CLI gates ran with the host's
stable toolchain; the Docker rows and Tauri host checks used the pinned Rust 1.90.0 + Node 22
container.

## 3. What works

- **Host add, connect, disconnect.** A profile is saved with a confirmed host-key fingerprint;
  connect runs over outbound SSH with `StrictHostKeyChecking=yes` and an app-private
  `known_hosts`; disconnect keeps the last view and marks it disconnected/stale. A changed key
  is a hard conflict, not an auto-accept. Source: `crates/spirit-core/src/ssh/`,
  `crates/spirit-core/src/host/`, D2/D3 commits.
- **Fleet canvas.** The VPS root, worker-slot ring, running worker cards with harness and
  provider/model badges, and the five job lanes render from the VOWP snapshot and queue
  documents. Job cards open the shared job drawer. Source: `packages/spirit-ui/src/views/fleet/`,
  D3 commits.
- **Capacity and storage rail.** Disk free versus the reserve floor, RAM available versus the
  reserve and p90 estimate, CPU load/pressure, target versus advised target, brake state,
  waiting reasons, and provider limits, with an explicit unavailable state when a document is
  missing. Source: `packages/spirit-ui/src/views/capacity/`, D3 commits.
- **Inbox and session metadata.** Open/answered/dismissed inbox items are readable, and running
  cards join to the newest session node/id. The standalone sessions drawer is not shipped; see
  §4. Source: `packages/spirit-ui/src/views/inbox/`, `docs/OWNER-VERIFICATION.md` §6.3.
- **D4 engine onboarding.** The pinned sidecar is bundled and started only from Tauri app
  resources, on `127.0.0.1` with an ephemeral port and a health probe. A model is labelled
  **Included · no key** only after the running engine reports its id. BYOK keys go through the
  OS-keychain boundary; external `opencode`/`claude`/`claude-code` installs are detected only
  after an explicit scan. Source: `crates/spirit-core/src/engine/`, D4 commits, and the D4
  onboarding report.
- **Gate cards.** `LocalOrchestratorGate` and `AsifGate` show probe evidence and the exact
  missing milestones. D4 keeps `features.asif_actions` and
  `features.local_orchestrator_actions` false, so a Ready probe still renders **not implemented
  in this build**. Source: `crates/spirit-core/src/engine/gate.rs`,
  `packages/spirit-ui/src/views/engine/`.
- **Shared UI.** `packages/spirit-ui` exports the design system, views, store, `SpiritBridge`,
  and `MockBridge` from TypeScript source. `apps/spirit-desktop/src/bridge/TauriBridge.ts`
  remains the only renderer file that imports `@tauri-apps/api`. Source: `packages/spirit-ui/`,
  `f4cb6fe`, desktop import-boundary test.
- **W1 read-only Web shell.** `apps/spirit-web` reads the daemon’s health, board, SSE stream,
  jobs, job detail/runs/gates, session metadata, and inbox routes. It shows capacity, supervisor,
  project state, and the standalone session/orchestrator surfaces as explicitly unavailable where
  the daemon has no route, has typed errors, reconnects SSE with backoff and a polling fallback,
  and never sends a token or a `POST`. Source: `docs/WEB.md` §9,
  `apps/spirit-web/src/bridge/HttpSseBridge.ts`, W1 commits.
- **C1 CLI.** `spirit host add|ls|show|remove|trust|probe|connect|disconnect`, `fleet`,
  `capacity`, `gates`, `engine status`, and `version`, with `--json`, fixture mode, and exit
  codes `0/2/3/4/5`. It reuses `spirit-core` and has no Tauri/React/desktop dependency. Source:
  `crates/spirit-cli/`, `crates/spirit-cli/README.md`, `858383c`.
- **Non-root Linux build.** `scripts/desktop-build-container.sh check` and `tauri-build` run the
  pinned Rust 1.90 + Node 22 + GTK/WebKit toolchain in a Docker container as the host UID/GID,
  with caches outside the worktree. Source: `docker/desktop-build.Dockerfile`,
  `scripts/desktop-build-container.sh`, `docs/DEV-SETUP.md`, `ec3c53f`.

## 4. Known limitations

- **macOS signature and notarization are not verified.** The owner checklist builds an unsigned
  debug `.app`; no Developer ID signature, notarization, stapling, or Gatekeeper distribution is
  covered. Source: `docs/OWNER-VERIFICATION.md` §6.3.
- **Windows signing is not verified.** Windows is optional for this MVP and no installer has
  been signed or tested. Source: `docs/OWNER-VERIFICATION.md` §2.4 and §6.3.
- **The real OS-keychain round trip is not covered on the worker/CI.** Tests use
  `FakeKeychain`; only a logged-in macOS, Windows, or Linux desktop session proves the real
  keychain. A headless Linux Secret Service is not covered. Source:
  `crates/spirit-core/src/engine/keychain.rs`, `docs/OWNER-VERIFICATION.md` §6.3.
- **The owner checklist has not been executed.** Every macOS build, functional, engine, and
  security step is pending. See §7.
- **Web daemon data gaps.** The Web shell has no capacity report (`GET /api/v1/capacity`), no
  supervisor state or project state (`GET /api/v1/fleet`), no standalone session/orchestrator
  surfaces, and no capacity-target write. Host profiles, SSH, the keychain, engine lifecycle,
  and gate probes have no daemon route and are not exposed by the Web shell; the desktop and CLI
  use the local store and SSH directly. Browser job/inbox writes are W1c and remain absent.
  Source: `docs/WEB.md` §8 and §9.7.
- **The shared desktop shell is not directly mounted by the Web app.** `FleetView` still requires
  non-null `capacity`/`supervisor`; the Web app composes a Web-specific shell over
  `WebFleetView` and the shared prop-driven views instead of fabricating desktop fields. The
  nullable-field follow-up is recorded in `docs/WEB.md` §9.6.
- **D5 `.asif.zip` sessions remain gated** on VOWP `v4-asif-core` and `v4-asif-cli`. The
  `AsifGate` stays disabled and `features.asif_actions` is false even if a probe says Ready.
- **D6 local orchestrators remain gated** on VOWP Phase 2 (`v4-local-2e/2f/2g`) and Phase 3
  (`v4-local-3*`). The `LocalOrchestratorGate` stays disabled and
  `features.local_orchestrator_actions` is false.
- **The standalone sessions drawer is not shipped.** Session metadata appears in the job drawer
  and worker card, but the separate drawer in `docs/PLAN.md` §8.6 must not be recorded as
  verified. Source: `docs/OWNER-VERIFICATION.md` §6.3.
- **The Linux container proves a host compile, not a release bundle or GUI.** It has no display
  server, no real credential store, and `tauri-build` uses `--no-bundle`, so no `.deb`/AppImage
  or visual inspection is covered. Source: `docs/DEV-SETUP.md` §“What remains a native, macOS,
  or CI check”.
- **Evidence-report caveat.** The per-job reports for the dependencies of this note were not
  available when it was written (only the D4 onboarding report was present). The gate results
  above are re-runs, not quotations from those missing reports.

## 5. Run and upgrade

### Desktop

```sh
npm ci
node scripts/fetch-engine.mjs                 # pinned, checksum-verified sidecar
(cd apps/spirit-desktop && npx tauri dev)     # development
(cd apps/spirit-desktop && npx tauri build --debug)
```

`npx tauri build --debug` validates `bundle.externalBin`; fetch the sidecar first if it is
missing. The engine binary and its generated lock file are gitignored. Full prerequisites,
per-platform commands, and expected output are in
[`docs/OWNER-VERIFICATION.md`](OWNER-VERIFICATION.md) and
[`docs/DEV-SETUP.md`](DEV-SETUP.md).

### Linux Docker check

```sh
scripts/desktop-build-container.sh check
scripts/desktop-build-container.sh tauri-build   # host compile + frontend, --no-bundle
```

Prerequisites: Docker Engine with a reachable daemon and a user that can run Docker without
`sudo`. Caches live outside the worktree; see [`docs/DEV-SETUP.md`](DEV-SETUP.md).

### CLI

```sh
cargo run -p spirit-cli -- --help
cargo run -p spirit-cli -- --fixtures fleet --json
cargo run -p spirit-cli -- --fixtures capacity --json
cargo run -p spirit-cli -- --fixtures gates --json
```

Storage, live host flow, fixture scenarios, and exit codes are in
[`crates/spirit-cli/README.md`](../crates/spirit-cli/README.md).

### Web

```sh
npm run dev:web                               # preview via MockBridge when VITE_SPIRIT_MOCK=1
```

For a live loopback daemon, the Vite dev server proxies `/api` to `VOWP_DAEMON_URL` while the
browser uses the dev-server origin through `VITE_VOWP_DAEMON_URL`; the exact commands and the
same-origin requirement are in [`docs/WEB.md`](WEB.md) §9.1. Do not put a daemon token in either
variable.

### Upgrade notes

Versions are pinned in `Cargo.lock`, `package-lock.json`, `engine/manifest.json`, and
`docker/desktop-build.Dockerfile`. Bump them together and re-run the gates in §2. The Sidecar
fetch is checksum-verified and refuses to overwrite a different binary without `--force`; never
commit the fetched binary or its lock file.

## 6. Security statement

- **Provider keys stay in the OS keychain.** The `Keychain`/`SecretStore` boundary stores keys
  through the platform credential store; `SecretString` wraps `Zeroizing`, has no `Debug`,
  `Display`, or `Serialize`, and keys are never written to the VPS, fixtures, logs, reports, or
  IPC payloads. The VPS receives provider and model names only. Source:
  `crates/spirit-core/src/engine/keychain.rs`, D4 commits.
- **No `.env` reads.** The repository contains no `.env` file and no StackSpirit source reads
  one; the core security test fails the build if `.env` appears in core source. The Web build
  consumes the documented `VITE_*` variables and the `VOWP_DAEMON_URL` proxy target. Source:
  `crates/spirit-core/tests/security.rs`.
- **No secret in logs, fixtures, or reports.** Logs are redacted and bounded; fixtures use
  documentation addresses and fake values; the repository secret scan is a release gate. The
  D4 report records fake test keys only.
- **Strict host-key pinning.** SSH runs with `StrictHostKeyChecking=yes`, `BatchMode=yes`, an
  app-private `UserKnownHostsFile`, and no auto-accept path. A changed key is a hard block with
  the old and new fingerprints. Source: `crates/spirit-core/src/ssh/mod.rs`,
  `crates/spirit-core/src/ssh/known_hosts.rs`.
- **Loopback-only engine and daemon.** The bundled engine starts on `127.0.0.1` with an
  ephemeral port; the VOWP daemon is loopback-only and requires a documented same-origin
  reverse proxy for any browser deployment. No StackSpirit surface binds a public listener.
  Source: `crates/spirit-core/src/engine/process.rs`, `docs/WEB.md` §5.
- **No browser token.** The read-only Web app has no token input, sends no `Authorization`
  header, and makes no `POST`. W1c writes, if built later, must go through a narrow server-side
  proxy. Source: `docs/WEB.md` §5, `apps/spirit-web/src/bridge/contract.test.ts`.

## 7. Owner sign-off status

**Status: pending. The `ss-9` owner checklist exists but has not been executed.** Its header
records “not executed” and its evidence table is empty. This note does not imply that the owner
ran any step.

Required evidence before the Desktop MVP can be called owner-verified (all are **pending**):

| Area | Checklist IDs | Evidence to attach |
|---|---|---|
| macOS build | M1–M7 | Redacted command output for `npm ci`, typecheck, lint, test, build, `npx tauri build --debug`, and a launched-window observation on macOS. |
| Linux build path | L1 native or L2 container | Redacted native-build output, or the container `check`/`tauri-build` output; the worker’s container run in §2 is worker evidence, not owner sign-off. |
| Functional smoke | F1–F10 | Add host, fingerprint confirmation, connect, fleet canvas, capacity rail, inbox, session metadata, disconnect/dimmed state, and the documented empty/unknown states against a real VOWP home. |
| Engine/D4 smoke | E1–E6 | First-run branding, included-model selection, BYOK save/verify/delete through the real OS keychain, external harness scan, gate-card waiting/ready states, and re-probe while connected. |
| Security | S1–S6 | No key in a log/URL/crash/fixture/report; changed host key is a hard block with old and new fingerprints; app-private `known_hosts`; no secret in a profile; VPS receives provider/model names only; no `.env` read. |
| Windows (optional) | W1 | Build/launch result or `N/A (optional for this MVP)`. |

**Web-specific status:** the Web e2e suite is **MockBridge-only**. No live `vowp-daemon` smoke
evidence exists in this worktree or in the available reports. A live daemon smoke is
**owner-run/manual and pending**; the MockBridge flows must not be recorded as live evidence.

Sign-off is valid only when all required steps in §2.2–§5 and at least one Linux build path
pass with redacted evidence and the app was run through the real Tauri bridge against a real
VOWP home (not Preview data), or when a written exception records the exact blocker, what was
attempted, the evidence, the owner/date, and the residual risk. Security checks S1–S3 are hard
gates. Source: [`docs/OWNER-VERIFICATION.md`](OWNER-VERIFICATION.md) §7.

## 8. Open follow-ups

1. **D5 live ASIF actions** — wait for VOWP `v4-asif-core` and `v4-asif-cli`; then prove
   `.asif.zip` export/import and the documented byte-count check (`docs/VOWP-CONTRACT.md`
   §11.2.2) before enabling `features.asif_actions`.
2. **D6 local orchestrators** — wait for VOWP Phase 2 (`v4-local-2e/2f/2g`) and Phase 3
   (`v4-local-3*`); `features.local_orchestrator_actions` stays false until the app host,
   bridge, MCP proxy, orchestrator materialization, backfill, and local cache are proven.
3. **Web capacity and supervisor routes** — `GET /api/v1/capacity` and `GET /api/v1/fleet`
   (supervisor, project state, sessions, orchestrators) are still daemon gaps; the Web shell
   keeps showing the documented unavailable copy.
4. **W1c write proxy** — browser job/inbox writes remain absent; they need the narrow
   server-side proxy in `docs/WEB.md` §5.4. The token-gated capacity-target write
   (`POST /api/v1/capacity/target`) remains a separate daemon/API gap and is not part of that
   allowlist.
5. **Shared UI nullability** — make `FleetView.capacity`/`supervisor` nullable, add the shared
   capability map, and export the pure job views and Web error titles from `packages/spirit-ui`.
6. **Owner verification** — execute `docs/OWNER-VERIFICATION.md` on macOS and at least one Linux
   path, attach redacted evidence, and record the sign-off block.
7. **Native distribution** — signed/notarized macOS and signed Windows installers remain
   future CI/release work; this note makes no packaging claim.
8. **Evidence audit** — the dependency job reports for `ss-5`, `ss-6`, `ss-7`, `ss-8`, `ss-9`,
   `ss-10`, `ss-11`, and `ss-16` were absent when this note was written. If the merge queue
   requires report-level provenance, recover them or accept the re-run evidence recorded in §2.
