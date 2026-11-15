# Owner verification — StackSpirit Desktop MVP (D1–D4)

> **Owner-run checklist.** Every build, GUI, SSH, VOWP, and OS-keychain step below is
> **owner-run**. None of them was executed on the Linux worker that wrote this document.
> A result recorded here is evidence from the owner's machine, not a worker claim.
>
> **Status when this document was written:** not executed. The evidence table in
> [§6](#6-evidence-template-and-known-limitations) is empty on purpose.

This checklist closes the platform gaps recorded by the D4 work: the Linux worker cannot
build or launch a macOS Tauri bundle, cannot open a GUI, and cannot exercise a real OS
keychain. It also gives the owner one place to record the manual smoke results that
automated tests cannot prove.

Related documents:

- [`docs/PLAN.md`](PLAN.md) §9.6 (D4), §10 (test strategy), §13 (Desktop MVP definition
  of done)
- [`docs/VOWP-CONTRACT.md`](VOWP-CONTRACT.md) §11 (gates, probe lifecycle, security)
- [`docs/DEV-SETUP.md`](DEV-SETUP.md) (Linux build container and what it does not cover)
- [`README.md`](../README.md) (engine packaging and development gates)

---

## 0. How to use this checklist

1. Work in order. Build first, then functional smoke, then the engine, then security.
2. Replace every `<placeholder>` locally. Never paste a real host, user, key, token,
   fingerprint, or absolute path into this file or into the evidence notes.
3. Record one row per step in the evidence table (§6). Use the step IDs below.
4. Allowed results: `PASS`, `FAIL`, `BLOCKED`, `N/A (reason)`, `NOT RUN`.
   - `PASS` means the expected result was observed in the release/debug build using the
     real bridge and a real VOWP home.
   - A result observed in **Preview data** mode (`VITE_SPIRIT_MOCK=1` or the MockBridge)
     is not owner verification. Stop and rerun from the packaged app.
5. Every required step names the evidence to save. Redact before saving.
6. Security checks are not optional. A key leak or an auto-accepted changed host key is
   an unconditional failure.

All commands assume the repository checkout is the current directory. The build commands
are copy-pasteable once you are there. Placeholders in angle brackets are not literal.

### 0.1 Local paths used below

Set `SPIRIT_APP_DATA` to the Tauri app-data directory for identifier
`com.stackspirit.desktop`. These are generic OS paths, not values copied from a machine:

```sh
# macOS
SPIRIT_APP_DATA="$HOME/Library/Application Support/com.stackspirit.desktop"

# Linux
SPIRIT_APP_DATA="${XDG_DATA_HOME:-$HOME/.local/share}/com.stackspirit.desktop"
```

On Windows the equivalent is `%APPDATA%\com.stackspirit.desktop`. The directory contains
`hosts.json`, `engine.json`, `known_hosts`, and `keychain-index.json`; the key values
themselves stay in the OS keychain.

---

## 1. Scope and prerequisites

### 1.1 In scope for this checklist

- D1–D4 Desktop MVP: host add/verify/connect/disconnect, fleet canvas, worker ring, job
  lanes, capacity rail, inbox, session metadata, StackSpirit Engine onboarding, included
  models, BYOK in the OS keychain, external harness detection, and the two VOWP capability
  gates.
- macOS is the **primary** target and every macOS step is required.
- Linux is **secondary**: one Linux build path is required, and the native desktop smoke
  is optional when a display server is available.
- Windows is **optional** for this MVP and marked as not required. Windows signing is out
  of scope.

### 1.2 Out of scope for this checklist

- A signed, notarized, or stapled macOS `.app`/`.dmg`.
- A signed Windows installer.
- A real OS-keychain round trip in CI (the worker/CI uses `FakeKeychain`).
- Live D5 `.asif.zip` export/import and live D6 local-orchestrator execution. Both remain
  gated on VOWP milestones and both action flags stay false in this build.
- The Web and CLI surfaces (`apps/spirit-web`, `crates/spirit-cli`). They have their own
  gates.
- Pixel-perfect visual regression and a full accessibility audit. Smoke-level keyboard
  access is checked; the automated axe suite remains in the repository gates.

### 1.3 Prerequisites

| Prerequisite | Requirement | Why |
|---|---|---|
| macOS | macOS 13 or newer, Apple silicon or Intel, Xcode Command Line Tools | Primary Tauri build and real OS keychain |
| Rust | Rust 1.85+ is the workspace MSRV; use **Rust 1.90.0** (or a newer 1.9x) | The pinned `Cargo.lock` (Tauri 2.12 and its locked dependencies) rejects 1.85; the Linux container pins 1.90.0 |
| Node | **Node 22.x LTS** (the container pins Node 22.23.5) and npm 10+ | Vite, Vitest, Playwright, Tauri CLI, and the engine fetch script |
| Tauri | Tauri 2 CLI, installed by `npm ci` through `@tauri-apps/cli` 2.12.x | `npx tauri build` |
| Linux packages | GTK 3, WebKitGTK 4.1, JavaScriptCore GTK 4.1, Soup 3 dev packages, or Docker | Native Linux host compile |
| SSH | SSH key or agent access to a real VOWP home | Live host add, fingerprint, connect, fleet, capacity, inbox |
| VOWP | `vowp` on the remote `PATH`, with a readable snapshot | Host probe and live data |
| Disposable test home | A throwaway VOWP home or test VPS | Inbox answer and changed-host-key checks are destructive |
| Provider key | One **throwaway** provider key, used only for the BYOK test | Real save/verify/delete against the OS keychain |
| Disk | About 1 GB for npm/Rust artifacts plus 65–80 MB for the fetched engine sidecar | Build and bundle |
| Network | GitHub release access for the first sidecar fetch; provider access only for BYOK verification | Fetch the pinned engine; verify a throwaway key |
| Docker (Linux alternative) | Docker Engine with a reachable daemon, about 4 GB free disk | `scripts/desktop-build-container.sh check` |
| `jq` (optional) | Any recent `jq`, used only by §5.4 | Inspect profile field names without values |

### 1.4 Safety rules

- **No `.env` file is read** by the app, tests, build scripts, or this checklist. Do not
  create one. Do not source one.
- **No real key is committed.** Use a throwaway provider key for BYOK and delete it after
  the test. The leak check in §5.1 uses a generated canary string, not a credential.
  StackSpirit never reads or stores an SSH private key; the host form accepts an identity
  file path only.
- Do not run the changed-host-key check or answer a real inbox item on a production VOWP
  home. Use a disposable test home.
- Keep evidence outside the repository checkout. Never commit screenshots, raw `hosts.json`,
  `known_hosts`, keychain exports, or terminal logs that contain a real value.
- The VPS receives provider/model **names only** in a later D6 build. D4 sends no engine
  data at all. A key value never crosses the SSH boundary in any build.

---

## 2. Build matrix

### 2.1 Common setup (all platforms)

Run from the repository root:

```sh
node --version
npm --version
rustc --version
cargo --version

TARGET_TRIPLE="$(rustc -vV | sed -n 's/^host: //p')"
printf 'target triple: %s\n' "$TARGET_TRIPLE"
node scripts/fetch-engine.mjs --target "$TARGET_TRIPLE"
bash scripts/verify-engine-checksum.sh
```

Expected:

- `node --version` prints `v22.x`.
- `rustc --version` and `cargo --version` print `1.90.x` (or newer 1.9x).
- The fetch ends with a line containing the gitignored sidecar path and a 64-character
  SHA-256, or says `already present and verified`. It exits `0`.
- The checksum script prints `verify-engine-checksum: opencode-<triple> ok` and
  `checked 1 sidecar(s)`, and exits `0`.

The sidecar is written to a gitignored path under
`apps/spirit-desktop/src-tauri/binaries/`. It is never committed.

**Why `--target "$(rustc -vV ...)"`:** on Apple silicon a Node process running under
Rosetta reports `x64`, but the Tauri build is `aarch64`. Passing the Rust host triple
keeps the sidecar architecture aligned with the app.

Failure recovery:

| Symptom | Recovery |
|---|---|
| `target ... is not pinned in engine/manifest.json` | Use the host triple printed by `rustc -vV`; the pinned targets are listed in `engine/manifest.json` |
| `checksum mismatch` | Delete the sidecar and `engine-lock.json` under the gitignored binaries directory, then fetch again. Never bypass the check |
| `could not run ... --version` | The archive is for the wrong OS/architecture, or the host cannot execute it. Refetch with the correct target |
| `no sha256sum or shasum on PATH` | Install `coreutils` (Linux) or use the macOS `shasum` that ships with the OS |
| `engine ... exists with an unexpected digest` | Pass `--force` only after confirming the existing file is a local artifact, not a pinned release |

### 2.2 macOS (primary, required, owner-run)

**Prerequisites:** Xcode Command Line Tools (`xcode-select --install`), Node 22, Rust
1.90.0, and the sidecar from §2.1.

```sh
npm ci
npm run typecheck
npm run lint
npm test
npm run build
(cd apps/spirit-desktop && npx tauri build --debug)
```

Expected results:

| Command | PASS looks like | Common failure and recovery |
|---|---|---|
| `npm ci` | Exit `0`; workspaces install; `package-lock.json` is unchanged | Node < 22 or a dirty lockfile: use Node 22 and do not run `npm install` |
| `npm run typecheck` | Exit `0`; no TypeScript errors | Install workspace dependencies first |
| `npm run lint` | Exit `0`; zero warnings (`--max-warnings 0`) | Fix the reported file; do not lower the limit |
| `npm test` | All shared-UI and desktop import-boundary suites pass | A real keychain or host is not needed; tests are hermetic |
| `npm run build` | Vite build succeeds; `apps/spirit-desktop/dist/` is written | Type errors or missing dependencies: rerun `npm ci` |
| `npx tauri build --debug` | Rust compiles, the sidecar is validated, and Tauri prints the final bundle path (usually `target/debug/bundle/macos/StackSpirit.app`) | Missing sidecar: §2.1. GTK/WebKit errors mean you are on Linux; see §2.3. `create-dmg` failure: rerun with `--bundles app` and record the DMG as out of scope |

The debug `.app` is **unsigned and not notarized**. That is expected for this checklist.
Do not record a signing or notarization result.

Launch the app:

```sh
open target/debug/bundle/macos/StackSpirit.app
```

If Tauri printed a different Cargo target directory, open the `.app` path from its final
`Finished ... bundle at:` line. Expected: a 1440×900 StackSpirit window opens, with no
boot error and the engine first-run or status view visible. A Gatekeeper prompt on a
locally built debug app is not a signature failure; use right-click **Open** once and
record it.

Optional UI-flow check (not required for sign-off):

```sh
(cd apps/spirit-desktop && npx playwright install chromium)
npm run test:e2e
```

Expected: the Playwright flows pass. Browser mode uses the MockBridge, so it is a
regression check, not owner evidence for the real host or keychain.

### 2.3 Linux (secondary, one path required, owner-run)

Use **Path A** (native packages) or **Path B** (container). Either path satisfies the Linux
build requirement. Path B is the reproducible CI path; Path A is a native owner build.

#### Path A — native Tauri packages

Debian/Ubuntu package set:

```sh
sudo apt-get update
sudo apt-get install -y build-essential curl wget file pkg-config libssl-dev \
  libgtk-3-dev libwebkit2gtk-4.1-dev libjavascriptcoregtk-4.1-dev \
  libsoup-3.0-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev patchelf
```

Expected: `apt-get` exits `0`, and `pkg-config --modversion gtk+-3.0 webkit2gtk-4.1`
prints versions. If you cannot install packages or do not have `sudo`, use Path B.

Then run:

```sh
TARGET_TRIPLE="$(rustc -vV | sed -n 's/^host: //p')"
node scripts/fetch-engine.mjs --target "$TARGET_TRIPLE"
bash scripts/verify-engine-checksum.sh

npm ci
npm run typecheck
npm run lint
npm test
npm run build
cargo check -p spirit-desktop
cargo test -p spirit-desktop
(cd apps/spirit-desktop && npx tauri build --debug --no-bundle)
```

Expected:

- `cargo check -p spirit-desktop` ends with `Finished dev profile`.
- `cargo test -p spirit-desktop` passes all host tests.
- `npx tauri build --debug --no-bundle` ends with
  `Built application at: .../target/debug/spirit-desktop` and exits `0`.
- No `.deb` or AppImage is produced by `--no-bundle`. That is intentional; packaged Linux
  bundles are CI-only for this MVP and are not required here.
- If a display server is available, launch the binary from a terminal and repeat the
  functional smoke. If not, record the Linux build as `PASS` and the GUI smoke as
  `N/A (no display)`.

#### Path B — pinned container

```sh
scripts/desktop-build-container.sh check
```

Expected final line:

```text
==> desktop-build: all desktop gates passed
```

Exit `0`. The command builds the image on first use, then runs the core and desktop gates
listed in [`docs/DEV-SETUP.md`](DEV-SETUP.md): formatting, engine fetch, `npm ci`,
typecheck, lint, tests, frontend build, `cargo check -p spirit-desktop`, and
`cargo test -p spirit-desktop`.

Optional extra host compile plus frontend bundle, without packaging:

```sh
scripts/desktop-build-container.sh tauri-build
```

Expected final line:

```text
==> desktop-build: tauri-build finished
```

and a `Built application at:` line for the debug host binary.

#### Which path proves what

| Evidence | Tauri host compile | UI gates | Real GUI / keychain | Package |
|---|---|---|---|---|
| Path A `cargo check`/`cargo test` + `tauri build --no-bundle` | **Yes** | Yes, from the `npm` commands | Only if a display and desktop session exist | No; packaging is CI-only |
| Path B `check` | **Yes** (`cargo check` and `cargo test` in the pinned image) | Yes | **No**; no display, no platform credential store | No |
| Path B `tauri-build` | **Yes** (real host compile + frontend bundle) | Frontend build only | **No** | No |
| Native `npx tauri build --debug` without `--no-bundle` | **Yes** | Yes | Only if launched | `.deb`/AppImage, CI-only/optional |

The container is the CI-grade Linux path. It does not prove a signed package, a real OS
keychain, or a visually inspected window.

### 2.4 Windows (optional, not required for this MVP, owner-run)

**NOT REQUIRED FOR THIS MVP.** Windows is listed for completeness. A Windows failure does
not block owner verification. Windows signing is out of scope.

Prerequisites: Windows 10/11, WebView2 Runtime, MSVC Build Tools, Rust MSVC toolchain, and
Node 22.

PowerShell, from the repository root:

```powershell
node scripts/fetch-engine.mjs
npm ci
npm run typecheck
npm run lint
npm test
npm run build
Set-Location apps/spirit-desktop
npx tauri build --debug
```

Expected: the host compiles and Tauri writes a debug executable and installer under
`target\debug\bundle\...`. It is unsigned. If the sidecar target is ambiguous, pass
`--target x86_64-pc-windows-msvc` (or the arm64 equivalent) to the fetch script.

If Windows is skipped, record `N/A (optional for this MVP)`.

---

## 3. Functional smoke

All functional steps are **owner-run**. Run them in the packaged/debug app built in §2.2
or §2.3. Never use Preview data.

### 3.1 Add a host and confirm the fingerprint

**F1. Add the VPS profile.**

- Do: open **Hosts** → **Add your VPS**. Fill **Label**, **Host**, **User**, **Port**,
  and optionally **Identity file**, **VOWP home**, and **Default project**.
  Use a disposable or test VOWP home. The identity field is a path only; StackSpirit never
  reads or stores the private key.
- Expected: invalid input is rejected inline when you click **Next: verify host key**
  (label 1–64 characters, host without a scheme/`@`/spaces, user token, port 1–65535,
  VOWP home absolute or empty, default project a slug or empty). The wizard stays on
  step 1 until the draft is valid.
- Evidence: redacted screenshot of step 1; note the validation cases you tried.

**F2. Confirm the host key out of band.**

- Do: compare the offered key with the server. On the server, run
  `ssh-keygen -lf /etc/ssh/ssh_host_ed25519_key.pub` (or the path to your host key) and
  compare the SHA-256 fingerprint. Select the matching key, then tick
  **I verified this fingerprint on my server**.
- Expected: every offered key is listed separately with algorithm, fingerprint, and a
  **preferred** badge where applicable. **Trust and continue** stays disabled until a key
  is selected and the checkbox is ticked. Multiple keys are never merged silently.
- Evidence: redacted screenshot; record the algorithm and a redacted fingerprint prefix.
  Do not record the full fingerprint or host name if that identifies the host.

**F3. Test and save.**

- Do: let the probe run. Expected checklist labels, in order: **Host key matches**,
  **SSH reachable**, **VOWP found**, **Snapshot readable**, **Supervisor running**.
- Expected: checks 1–4 show **Passed**; **Save host** is enabled. Check 5 is
  informational: a stopped supervisor still allows save and connect, and the host card
  then reads **Connected · supervisor stopped**. The toast says
  `<label> saved. Connect when you are ready.` The host card shows the label,
  `user@host:port`, last connected, host-key algorithm, and VOWP home.
- Evidence: redacted screenshot of the checklist and host card; record the five stages.

Failure recovery:

| Symptom | Recovery |
|---|---|
| SSH unreachable | Check the host, port, SSH agent/key, and network. The form values are preserved |
| Host key mismatch | Stop. Follow §5.2; do not re-pin until the new fingerprint is verified out of band |
| VOWP not found | Run `vowp --version` on the host and fix the remote `PATH` |
| Snapshot unreadable | Run `vowp --json fleet status` manually; check the VOWP version and home path |
| Supervisor stopped | Informational only; save and connect, then expect the stopped-supervisor copy |

### 3.2 Connect

**F4. Connect to the saved host.**

- Do: select the host and click **Connect** in the top bar (or **Connect** on the host
  card).
- Expected: the top bar shows **Connecting…**, then **Connected** with a green status dot,
  the host label, `user@host:port`, and a latency value with a sparkline. The supervisor
  chip reads **supervisor up · tick Ns**, **supervisor stopped**, **supervisor paused**, or
  **supervisor quiet · Ns**. The fleet canvas and capacity rail populate from the first
  snapshot.
- If another host is already connected: the prompt is **Disconnect and switch host?** with
  **Keep current host** and **Disconnect and connect**. Only one host is connected at a
  time.
- Evidence: redacted screenshot of the top bar and first snapshot; record the supervisor
  chip and latency.

Failure recovery: authentication error → check the SSH agent and identity path; host-key
error → §5.2; VOWP error → rerun the host probe; no snapshot → check the VOWP home and
version.

### 3.3 Fleet canvas, worker ring, and job lanes

**F5. Read the fleet story without opening a terminal.**

- VPS root card: **VPS root**, the VOWP home (redact), a supervisor badge, **Projects**,
  **Running**, **Queue**, **Limits**, and `VOWP <version>` plus `last tick ... ago`.
- Worker ring: center shows `<busy> / <target> workers`; below it the accessible summary
  `<target> worker slots, <busy> busy, <idle> idle, target advised by VOWP is <n>, <n>
  other runs.` The ring draws up to the target, shows over-target and stalled badges, and
  shows verifier runs separately. A target of 0 says **No workers allowed — VOWP target is
  0** and still shows any visible worker cards honestly.
- Running workers: header `<n> visible`; each card shows job id, kind, title when known,
  executor badge (harness · provider/model · variant when available), state
  **live**/**quiet**/**stalled**, and a short session id when present. A shell/gate/merge
  run with no executor shows a **shell** badge and no model chip. If the title is missing,
  the card shows the job id and a **title unavailable in snapshot** tooltip; it never
  invents a title.
- Job lanes: exactly five sections — **Running**, **Ready**, **Review & Fix**,
  **Parked & Blocked**, **Done**. Header counts come from `jobs_by_state`, not from the
  visible cards. Empty lanes say **No jobs**; the Done lane explains that it is summarized
  from `jobs_by_state`. If VOWP truncates the queue, the lane says
  **showing the first 200 jobs**.
- Project switcher: **All projects** is the default; selecting a project filters lanes and
  worker cards but not the node-wide capacity rail.
- Unknown job state: the chip reads **Unknown** and the raw state remains available in the
  tooltip/drawer; it is never labeled as running, ready, or done.

Expected: every value above is visible and matches the last VOWP snapshot. Nothing is
blank merely because the view has not loaded, and nothing is invented when VOWP omits it.

Evidence: redacted full-canvas screenshot; record the ring summary, lane counts, and any
honest empty/unknown states from §3.8. If a lane count disagrees with the cards, record the
snapshot raw drawer and fail the check.

### 3.4 Capacity and VPS storage rail

**F6. Read the capacity rail and the raw-document drawer.**

- Brake: either **No brake active** with “VOWP is admitting runs subject to target and
  provider limits”, or **VOWP brake** with **brake active**, its since time, and each
  reason. New runs wait; running runs continue.
- Target advisor: current **Worker target**, **Mode** (`Auto`/`Fixed`), and
  **VOWP advises N** or **VOWP has no advice yet**. The advice action is
  **Apply advised** when there is headroom and **Match advised** when the target is above
  advice; it is disabled when there is no advice. Setting a target above advice opens
  **Confirm owner override** and requires an explicit confirmation.
- Home disk: **Home disk free** against the floor, a source badge
  (**capacity report**, **snapshot**, or **no sample**), the state copy, and **Unknown**
  when no host sample exists. A separate **Worktrees free** gauge appears only when VOWP
  reports a different worktree value.
- Memory: **Memory available** against the reserve and running p90. When the capacity
  report is loaded, show the per-worker p90 with source and samples, or **Not enough
  samples for a per-worker estimate.** When it is not loaded, show **Capacity detail
  unavailable — retrying.** Do not invent a per-worker estimate.
- CPU: **CPU load** as load1 / core count, with **Pressure CPU avg10** or the exact
  **not reported** text. Do not treat missing pressure as zero.
- Waiting reasons: grouped labels **RAM**, **CPU**, **Disk**, **Provider limit**,
  **Brake**, **Target**. Empty says **No jobs are waiting for capacity.**; unloaded says
  **Capacity detail unavailable — retrying.**
- Provider limits and caps: separate from disk/RAM/CPU. Empty says **No provider limits
  reported.** Provider caps appear only when the capacity report is loaded; otherwise the
  panel says the caps load with the report.
- Freshness footer: `Capacity data from VOWP · HH:MM:SS · fresh` or
  `· stale · retrying in Ns`, and **capacity detail unavailable — retrying** when the
  report is missing. Clicking it opens the read-only **Raw VOWP document** drawer with
  `generated_at`, `fetched_at`, source, parse warnings, and parsed documents.

Expected: each panel honestly reports its source and unknown/empty state. A capacity
report failure must not blank the last good gauges.

Evidence: redacted screenshots of the rail and the raw drawer; record disk free/floor, RAM
available/reserve, CPU pressure, target/advice, brake state, and limits.

### 3.5 Inbox

**F7. Open the inbox and answer only on a disposable home.**

- Do: click the inbox bell. The drawer shows **Open** and **All** tabs with counts.
- If an open item exists on a disposable test home: select it, read the body and suggested
  options, type an answer, and click **Send answer**. Expected: VOWP receives the answer,
  the item moves to **All** as **answered**, the open count decrements, and the snapshot
  refreshes. The job does not move optimistically.
- If no open item exists: expected **Inbox is clear** (with “Questions and review requests
  from VOWP appear here.”) or **No inbox items** on the All tab. Do not create an item on a
  production home.
- If the connection is stale or disconnected, the drawer shows a warning and answer
  controls are disabled with **Refresh before answering.**

Evidence: redacted screenshot; record the open count and whether an answer was sent. If no
disposable item exists, record `N/A (no disposable inbox item)` rather than inventing one.

### 3.6 Sessions

**F8. Read session metadata without claiming a live terminal.**

- Do: select a running worker card or a job card, open the job drawer, and read
  **Executor and session**. The worker card also shows a short session id when VOWP
  reports one.
- Expected: the raw executor and display string are shown when present; otherwise
  “No executor recorded (shell, gate or merge run).” **Session:** shows the id or
  **not reported**. There is no live terminal and no log streaming.
- This build exposes session metadata in the job drawer and worker card; it does **not**
  ship a standalone sessions drawer from `docs/PLAN.md` §8.6. Do not record a standalone
  sessions drawer as verified. The `.asif.zip sessions` gate card in §4.5 covers the
  gated session-package surface.
- Evidence: redacted screenshot of the **Executor and session** section; record whether a
  session id was present.

### 3.7 Disconnect, dimmed state, and stale state

**F9. Disconnect.**

- Do: click **Disconnect** in the top bar (or the host card).
- Expected: **Disconnecting…**, then **Not connected**. The last fleet canvas remains
  visible but is dimmed, with the notice **disconnected · Showing data from HH:MM:SS.
  Fleet actions are disabled until you reconnect.** Capacity remains visible but dimmed;
  its freshness footer is not a live reading. Mutation controls are disabled: the target
  editor, job actions, inbox answers, and fleet refresh. The status strip shows the
  disconnected state and the last source/generated/fetched values.
- Evidence: before/after screenshots; record the exact notice and the disabled controls.

**F10. Stale state (only if you can safely interrupt the test host).**

- Do: while connected, interrupt connectivity to the test VOWP home (for example, stop
  the test SSH server) and wait for three failed polls, or until the app shows **Stale**.
- Expected: amber status, **Stale**, last data dimmed, **stale**/retrying copy, and writes
  disabled until a refresh succeeds. If the transport closes instead, the app shows
  **Failed**; that is also honest, but it must not enable writes. Reconnect or restart the
  test SSH server; the next successful poll clears the state.
- Do not run this on a production host. If you cannot interrupt safely, record
  `N/A (no safe way to force stale)`; do not claim it.

### 3.8 Expected honest empty and unknown states

Use this table when checking F5–F9. An empty or unknown state is correct only when it uses
the specified language and does not invent a value.

| Situation | Where | Expected honest state |
|---|---|---|
| No saved host | Fleet / Hosts / Capacity | **No fleet snapshot yet**, **Add your VPS**, **Capacity unavailable** |
| Disconnected with last data | Fleet | **disconnected · Showing data from … Fleet actions are disabled until you reconnect**; canvas dimmed |
| Supervisor stopped | Top bar / VPS root | **supervisor stopped**; the save wizard also warns **Connected · supervisor stopped**; no invented running state |
| No running workers | Running workers | **No running workers** |
| Empty lane | Each lane | **No jobs**; Done uses its summary copy |
| Queue truncated | Ready/Parked lanes | **showing the first 200 jobs** |
| Capacity report absent | Rail | **Snapshot capacity; the detailed report is not loaded.**; detail panels say **Capacity detail unavailable — retrying.** |
| No host sample | Disk/RAM/CPU | Gauge **Unknown** and the matching state copy (**No disk sample yet.** etc.) |
| No RAM per-worker estimate | Memory | **Not enough samples for a per-worker estimate.**; do not invent slots |
| No target advice | Target advisor | **VOWP has no advice yet**; **Apply advised** disabled |
| No provider limits | Limits | **No provider limits reported.** |
| No provider caps | Limits | Caps load with the capacity report; hide them rather than guessing |
| No inbox items | Inbox | **Inbox is clear** / **No inbox items** |
| No included models | Engine | **This engine build has no included models available. Add your own provider key to continue.** |
| No session id | Job drawer / worker card | **not reported** |
| Unknown job state | Lanes / job card | **Unknown** chip; raw state kept in tooltip/drawer |
| No external harness | Engine | **No existing harness found. StackSpirit scans only the exact names opencode, claude and claude-code on PATH; it does not walk the disk.** |
| Gates not probed | Engine | **Capability gates not probed**; **Probe gates** disabled with a reason |
| Engine unavailable | Engine | **Engine status unavailable** with **Refresh status**; no fake ready state |

---

## 4. Engine and D4 smoke

All engine steps are **owner-run** and use the real OS keychain. Preview fixtures are not
evidence.

### 4.1 First run and branding

**E1. Verify first-run language and engine readiness.**

- Do: use a fresh app-data/profile state for the first run. If you have already completed
  setup, use a second OS user or back up and move the app-data directory only after
  reading §6.3; do not delete a profile you still need. Do not open **Advanced details**
  until the branding check is complete.
- Expected on steps 1–3:

  1. **Engine check** panel title **StackSpirit Engine**; the first-run heading
     **StackSpirit Engine check**; when healthy, “StackSpirit Engine found · version
     <version> · `127.0.0.1:<port>`”.
  2. **Choose models** with **Included models (recommended)**, **Use my own provider key**,
     and **Use an existing harness**.
  3. **Finish** with **Model selected · <label>** and **Finish setup**.

  The first-run screens contain no “opencode”, no “Install opencode”, and no “Configure
  opencode”. After setup, “opencode” may appear only under **Advanced details** (“Engine
  implementation: opencode <version>”), the **About**/license notice, and the external
  harness context. That is the language rule from `docs/PLAN.md` §8.5.1.

- Evidence: screenshots of each first-run step; record “no opencode on steps 1–3” and
  the healthy engine version/loopback line.

### 4.2 Included models

**E2. Select an included model and verify persistence.**

- Do: on **Choose models**, leave **Included models (recommended)** selected. Inspect each
  row: label, provider, context window, **Included · no key**, and
  `verified <time>` after runtime verification. Select a model and **Continue**.
- Expected: only runtime-verified included models are offered; a BYOK-only/paid model is
  not offered as included. If the engine reports no included models, the empty state
  above appears with **Add your own provider key** and nothing is auto-selected. On
  **Finish**, the selected model is named. After **Finish setup**, the Engine status card
  shows the selected model. Relaunching the app preserves the selection in local
  `engine.json` and does not show first-run again.
- Evidence: screenshot of the model list and status card; record the selected model label
  and the “verified” state. Do not record a key.

### 4.3 BYOK save, verify, and delete

**E3. Save a throwaway key, verify it, and delete it.**

Use one **throwaway** provider key only. Never use a production key. The provider list is
derived from the bundled engine’s supported providers, not a fixed UI list.

- Do:
  1. Open **Engine** → **Change model** (or first-run step 2) → **Use my own provider key**.
  2. Select a provider, paste the throwaway key into the password field, and click
     **Save**.
  3. Observe the status. In this build, Save also triggers verification; there is no
     separate Verify button. The key field clears immediately.
  4. Open the OS keychain UI and confirm the provider entry exists under service
     `com.stackspirit.desktop`. Do not reveal or copy the password.
  5. Relaunch the app and confirm the provider status is still present and no key is
     displayed.
  6. Click **Delete key**, confirm **Delete provider key?** with **Delete key**, and
     confirm the entry is gone.
- Expected:
  - Status is one of **Saved**, **Invalid**, or **Unverified**. **Saved** means the local
    engine accepted the key; **Invalid** means it rejected it; **Unverified** means the
    engine could not prove it. A failed verification does not delete the key.
  - The key is never echoed, logged, placed in a URL, or shown as a prefix. The UI shows
    provider names and states only.
  - The keychain entry exists after Save and is gone after Delete. The engine/model
    selection is unaffected by Delete.
  - No key is written to `hosts.json`, `engine.json`, `known_hosts`, fixtures, or the
    evidence report. The provider-name index may contain the provider name only.
- Evidence: redacted screenshots of the status badge and keychain entry name; record
  `Saved`/`Invalid`/`Unverified` and whether Delete removed the entry. If verification is
  `Invalid` or `Unverified`, record why; if no valid throwaway key was available, mark the
  verification sub-check `BLOCKED (no throwaway key)` and the storage/delete sub-check
  separately.

Failure recovery: locked keychain → unlock the login keychain and retry; Linux without a
Secret Service session → use a real desktop session or record the exact blocker; delete
failure → retry after unlocking, and check that only the provider entry, not other
credentials, was targeted.

### 4.4 External harness scan

**E4. Scan PATH only when asked, and select explicitly.**

- Do: record what is on `PATH` first:

  ```sh
  for name in opencode claude claude-code; do
    which -a "$name" 2>/dev/null || true
  done
  ```

  Then open **Engine** → **Existing harness** → **Scan PATH**.
- Expected: results contain only exact names `opencode`, `claude`, and `claude-code` (with
  `claude` and `claude-code` represented as Claude Code). No disk walk, no other binary,
  no alias/function. Each row shows kind, absolute path (redact), **Version:** value or
  **not reported**, and **Usable**/**Not usable**. **Use this installation** is enabled
  only for a usable installation. If none is found, the exact empty copy from §3.8
  appears.
- Do not create or edit the external tool’s configuration. Selecting **Use this
  installation** must stop the bundled sidecar, show **External** in the Engine view and
  **External: opencode**/**External: Claude Code** in the top bar, and disable bundled-only
  controls such as **Restart engine**. **Use bundled engine** must be an explicit action;
  there is no silent fallback.
- Evidence: redacted screenshot of the scan result and external state; record the names
  found and whether the external config timestamp was unchanged.

Failure recovery: no harness → expected empty state, not a failure; a harness listed but
**Not usable** → its `--version` failed, select it only if you accept that; wrong names
listed → failure of the exact-name rule; external selection starts the bundled sidecar →
failure of the no-silent-fallback rule.

### 4.5 Gate cards: waiting/ready, exact missing milestones, disabled actions

**E5. Inspect both gate cards and their Details drawers.**

- Do: open **Engine** → **Capability gates** with a live connection. The app auto-probes on
  connect/first view; click **Re-probe** if needed. Open **Details** on each card.
- Expected on the current VOWP home: both cards show **Waiting**, not a green **Ready**.
  If the probe cannot reach the home, expect **Unreachable** with a retry hint; reconnect
  and re-probe before recording a result.
  - **Local orchestrator** (`LocalOrchestratorGate`): description “Run global and project
    orchestrators on this Mac.”; detail `Waiting for VOWP Phase 2/3 · detected <version>`;
    a **Missing:** line listing **exactly the probe’s missing capabilities** (not a
    hardcoded superset). The gating milestones are Phase 2 (`v4-local-2e/2f/2g`) and
    Phase 3 (`v4-local-3*`). The full Phase 3 capability set is
    **local opencode app host**, **surface bridge**, **MCP proxy**,
    **orchestrator materialization**, **backfill**, and **local cache**. A home that
    proves none may report all six; a home that proves some must report only the unproven
    subset (the probe may also report an earlier missing node/offload label).
  - **.asif.zip sessions** (`AsifGate`): description “Download and import portable session
    packages.”; detail `Waiting for VOWP ASIF · detected <version>`; missing
    **v4-asif-core** and **v4-asif-cli**.
  - Actions **Run locally**, **Export .asif.zip**, and **Import .asif.zip** are disabled.
    The adjacent reason is visible, not only a tooltip. On a waiting gate it names the
    missing milestones. On a **Ready** gate it must still say **Not implemented in this
    build** while the D4 feature flags are false (`features.local_orchestrator_actions`
    and `features.asif_actions`). A green gate never enables a dead action.
  - **Details** shows the state badge, detected VOWP version, `probed <time>`, the exact
    missing list, probe evidence (commands, exit codes, versions only — no response body,
    path, key, or token), the local-agent state for the local gate, and the “not
    implemented in this build” explanation.
- If your VOWP home reports **Ready**: verify both action buttons remain disabled and the
  reason says not implemented in this build. This is the correct D4 state; D5/D6 flip the
  corresponding flag only when their action ships. If your home is waiting, do not fake a
  Ready result; the Ready+disabled path is covered by the automated D4 suite.
- Evidence: screenshots of both cards, the disabled reasons, and both Details drawers;
  record the exact missing list and the `probed` time.

Failure recovery: a gate shows **Ready** without probe evidence → fail; an action is
enabled while its feature flag is false → fail; a missing line lists capabilities the probe
did not report as missing → fail; a gate blocks fleet/capacity/inbox/engine while waiting →
fail (the gate must not block the rest of the app).

### 4.6 Re-probe only while connected

**E6. Verify the connection requirement.**

- Do: disconnect from the VOWP home and open **Engine** → **Capability gates**. Then
  reconnect and probe again.
- Expected while disconnected or stale: the view shows **Capability gates not probed**
  (or the last snapshot is hidden), **Re-probe**/**Probe gates** are disabled, and the
  visible reason is **Connect to a VOWP home before probing capability gates.** or
  **The VOWP connection is stale. Reconnect before probing capability gates.**
- Expected while connected: **Re-probe** is enabled; clicking it shows **Probing**, then a
  new result with a new `probed <time>` in Details. The result is scoped to this
  connection: disconnecting and reconnecting must not display the previous host’s Ready
  state.
- Evidence: screenshots of the disabled and enabled states; record both `probed` times.

Failure recovery: Re-probe enabled while disconnected/stale → fail; a cached Ready from a
previous host shown after reconnect → fail; Re-probe returns without a new time → record
the error and retry once.

---

## 5. Security checks

These checks are **owner-run** on the built app and a real desktop session. Do not run the
destructive host-key check on a production home.

### 5.1 No provider key in a log, URL, crash output, fixture, or report

**S1. Use a generated canary, not a credential.**

- Do: generate a local canary and save it through **Use my own provider key**:

  ```sh
  CANARY="stackspirit-owner-$(uuidgen 2>/dev/null || date +%s)"
  printf '%s\n' "$CANARY"
  ```

  Paste the canary as the provider key. It is intentionally not a real key; **Invalid** or
  **Unverified** is the expected verification outcome. Then:
  1. Open **View logs** in the Engine view and the onboarding failure log path; none may
     contain the canary.
  2. Search the app-data directory:

     ```sh
     grep -RIIn -F "$CANARY" "$SPIRIT_APP_DATA" || true
     ```

  3. If a crash occurred, search the macOS diagnostic reports:

     ```sh
     grep -RIl -F "$CANARY" "$HOME/Library/Logs/DiagnosticReports" 2>/dev/null || true
     ```

  4. Search the checkout for the canary to prove it did not enter fixtures or a report:

     ```sh
     grep -RIIn -F --exclude-dir=node_modules --exclude-dir=target \
       --exclude-dir=.git "$CANARY" . || true
     ```

  5. Inspect the Engine details: the only engine URL is `http://127.0.0.1:<port>` with no
     query string. Any key in a URL, path, query, or log is a failure.
  6. Delete the canary entry with **Delete key** when finished.

- Expected: no output from the searches (the only persistent copy while saved is the OS
  keychain entry); no canary in logs, URLs, crash reports, fixtures, or the evidence
  report. The UI never prints the key or a prefix.
- Evidence: command output showing `no match` (redact the canary itself if it appears in a
  prompt), screenshots of the log/details panels, and a note that Delete removed the entry.
- Note: `SPIRIT_APP_DATA` is the Tauri app-data directory for identifier
  `com.stackspirit.desktop` (§0.1). The OS keychain is outside that directory, so the
  absence of the canary there is expected and is the proof that no profile copy exists.

### 5.2 A changed host key is a hard block with old and new fingerprints

**S2. Destructive; disposable test home only.**

- Do:
  1. Connect normally once and confirm the pin.
  2. Disconnect.
  3. On the disposable test server, regenerate its SSH host key (or swap in a second test
     server with a different key on the same `host:port`) and restart the test SSH server.
  4. Connect to the saved profile again.
- Expected: the modal **Host key changed** appears; the text says StackSpirit refused to
  connect; **Pinned** and **Server offered** show two different SHA-256 fingerprints;
  the footer actions are **Do not connect** and **Re-pin key…**. Dismissing the dialog
  leaves the app disconnected. There is no “continue anyway”, no auto-accept, and no
  hidden bypass. The connection remains blocked. **Re-pin key…** opens the fingerprint
  flow, which requires a rescan and the explicit
  **I verified this fingerprint on my server** checkbox before the pin changes.
- Click **Do not connect** and confirm the app stays disconnected. Restore the test
  server or complete an intentional re-pin only after out-of-band verification.
- Evidence: redacted screenshot showing both fingerprint labels (record only a short
  prefix/suffix); record that the connection was blocked.

Failure recovery: if the app connects without the modal, stop and fail the check; if only
one fingerprint is shown, fail; if **Re-pin** skips the checkbox, fail.

### 5.3 The app-private `known_hosts` is used

**S3. Verify file ownership and location.**

- Do:

  ```sh
  SPIRIT_APP_DATA="${SPIRIT_APP_DATA:-$HOME/Library/Application Support/com.stackspirit.desktop}"
  stat -f '%Sp %N' "$SPIRIT_APP_DATA/known_hosts"
  ssh-keygen -f "$SPIRIT_APP_DATA/known_hosts" -F '[<vps-host>]:<port>' || \
    ssh-keygen -f "$SPIRIT_APP_DATA/known_hosts" -F '<vps-host>'
  ```

  On Linux use `stat -c '%a %n'` and
  `SPIRIT_APP_DATA="${XDG_DATA_HOME:-$HOME/.local/share}/com.stackspirit.desktop"`.
- Expected: the app-private file exists with mode `-rw-------` (0600). `ssh-keygen -F`
  prints one host-key line for the saved host/port. StackSpirit must not add a new entry
  to your global `~/.ssh/known_hosts`; an entry that was already there from your own SSH
  use is unrelated. Tight host-key checking is proved by §5.2: a missing or changed pin
  fails closed.
- Evidence: `stat` output and redacted `ssh-keygen -F` output; a note that the global
  file was not modified by StackSpirit.

Failure recovery: mode is not 0600 → fail; the global file gained the entry → fail; no
app-private entry but connection works → fail (the app is not using the private pin file).

### 5.4 No secret is stored in a profile

**S4. Inspect profile field names, not values.**

- Do: with `SPIRIT_APP_DATA` set as in §5.3:

  ```sh
  : "${CANARY:?Set CANARY with the S1 canary first; do not grep with an empty pattern}"
  for f in hosts.json engine.json keychain-index.json; do
    printf '%s: ' "$f"
    jq -r 'if type == "array" then join(",") else keys | join(",") end' \
      "$SPIRIT_APP_DATA/$f" 2>/dev/null || echo 'not present'
  done
  grep -RIIn -F "$CANARY" "$SPIRIT_APP_DATA" || true
  ```

- Expected: `hosts.json` contains host profile metadata and a host-key pin (algorithm,
  fingerprint, pinned time), never a private key. `engine.json` contains the engine choice,
  selected model id, and onboarding flag, never a key. `keychain-index.json` contains
  provider names only. `known_hosts` contains public host keys, not private keys. The
  canary grep returns no match.
- Evidence: field-name output (values redacted) and the no-match result.

Failure recovery: any `secret`, `api_key`, `token`, or key-like value in a profile → fail;
a canary match in app data → fail and delete the entry.

### 5.5 The VPS receives provider/model names only

**S5. In D4, no engine data crosses the SSH boundary.**

- Do:
  1. Disconnect from the VOWP home and complete engine setup (included model or BYOK)
     while disconnected. Expected: the engine starts and the model/keychain operations
     work locally; no VPS is required.
  2. Reconnect and perform fleet/capacity/inbox reads. Expected: only the documented read
     commands run (`vowp --json fleet status`, `vowp --json capacity report`,
     `vowp --json fleet queue`, `vowp --json fleet inbox --all`); none of them takes a
     provider key or an engine model. Engine setup and model selection make no VOWP write.
  3. On a disposable home, compare the VOWP configuration before and after the engine
     steps. No provider/model entry may be added by StackSpirit. VOWP may write its own
     runtime state; distinguish that from a profile/config change.
- Expected: D4 sends **no** engine/provider/model data to the VPS. In a later D6 build,
  only provider/model **names** may cross; a key value never does. A provider key in a
  remote file, command argument, URL, or log is an unconditional failure.
- Evidence: note that engine setup worked while disconnected; redacted config diff or a
  statement that no config change occurred. Mark the D6 names-only path `N/A (D6 gated)`
  rather than pretending to test an unshipped action.

### 5.6 No `.env` file is read

**S6. Confirm the build and app need no environment file.**

- Do:

  ```sh
  find . -path './node_modules' -prune -o \( -name '.env' -o -name '.env.*' \) -print
  ```

  Run the build and app with no `.env` present. Do not create one for this checklist.
- Expected: no `.env` file is required, read, or committed. Build commands and the app
  behave normally. Provider keys are entered only in the password field and stored only in
  the OS keychain.
- Evidence: command output and a note that no `.env` was created.

---

## 6. Evidence template and known limitations

### 6.1 Evidence table

Copy this table into a local evidence note (outside the repository checkout). Add one row
per step. The example row is a shape, not a result.

| Date | Platform | Command / step | Result | Evidence path or note |
|---|---|---|---|---|
| `<YYYY-MM-DD>` | `macOS <version> <arch>` | `M1` `npm ci` | `PASS` | `owner-evidence/<date>-macos-npm-ci.txt` |
| | | | | |
| | | | | |

Suggested IDs: `M1`–`M7` for §2.2, `L1`/`L2` for §2.3 Path A/B, `W1` for §2.4,
`F1`–`F10` for §3, `E1`–`E6` for §4, `S1`–`S6` for §5.

**Redaction reminder:** before saving any evidence, replace the real host name/IP with
`<vps-host>`, the SSH user with `<ssh-user>`, absolute paths with `<path>`, fingerprints
with a short prefix or `<redacted>`, and every key/token/canary with `<redacted>`. Never
paste a key, a full credential, a private key, or a password into this document, a
screenshot, a report, or a commit.

### 6.2 Evidence storage

Keep evidence outside the repository checkout, for example in an encrypted local folder
or a private note. Do not commit screenshots, `hosts.json`, `known_hosts`, keychain
exports, terminal transcripts, or crash reports. The `.vowp` directory is local and
ignored by this repository, but that is not a license to store secrets there.

### 6.3 Known limitations

- **macOS signing/notarization:** the debug `.app` is unsigned; this checklist does not
  verify a Developer ID signature, notarization, stapling, or Gatekeeper distribution.
- **Windows signing:** not verified. Windows is optional for this MVP and its installer is
  unsigned.
- **Real OS keychain in CI:** the worker/CI uses `FakeKeychain`; only a logged-in macOS,
  Windows, or Linux desktop session proves the real keychain. A headless Linux Secret
  Service is not covered.
- **Live D5/D6 actions:** `.asif.zip` export/import and local-orchestrator execution are
  gated on VOWP milestones. Both action flags are false; the gate cards stay disabled even
  when a probe says Ready. This checklist does not test the live actions.
- **Standalone sessions drawer:** this build exposes session metadata in the job drawer
  and worker card; it does not ship the separate sessions drawer described in
  `docs/PLAN.md` §8.6. Do not record that drawer as verified.
- **Linux packaging and visual inspection:** the container has no display or platform
  credential store. It proves the host compile and UI gates, not a packaged `.deb`/AppImage,
  a visual window, or a keychain round trip. Packaged Linux bundles remain CI-only.
- **Gate Ready:** a release build cannot force Ready without real VOWP capability. Current
  VOWP is expected to show Waiting. The Ready+disabled path is covered by the automated D4
  suite, not by this owner smoke.
- **Inbox answer:** requires a disposable home with an open item. Without one, record
  `N/A (no disposable inbox item)`; do not create data in a production home.
- **Changed-host-key check:** requires a disposable test server. It is intentionally
  destructive and is not run against production.
- **Provider verification:** a BYOK key that is invalid or unverified still tests keychain
  storage and deletion, but not provider acceptance. Record the verification sub-check
  separately and do not call an `Invalid` key a successful verification.
- **App-data reset:** first-run verification may require a fresh profile. Moving or
  deleting the app-data directory does not delete OS keychain entries; delete test keys
  explicitly and restore any profile you still need.

---

## 7. Pass/fail rule and sign-off

### 7.1 Pass/fail rule

The Desktop MVP is **owner-verified** only when all of the following are true:

1. Every required macOS step in §2.2, §3, §4, and §5 is `PASS` (or a documented `N/A`
   where this checklist explicitly allows it, such as the inbox answer with no disposable
   item).
2. **At least one** Linux build path in §2.3 (`L1` native or `L2` container) is `PASS`.
3. Every result has a redacted evidence row, and the evidence does not contain a real
   host, key, token, private key, or secret.
4. Any exception is written down with the exact blocker, what was attempted, the evidence,
   the owner/date, and the residual risk. An exception records a gap; it does **not**
   convert a failed check into a pass.
5. The app was run through the real Tauri bridge and a real VOWP home, not Preview data.

Additional rules:

- **No signed/notarized bundle is required for this checklist.** Do not fail a pass because
  the debug build is unsigned.
- **Windows is not required.** Record `N/A (optional for this MVP)` if skipped.
- **Security checks S1, S2, and S3 are hard gates.** A key leak, an auto-accepted changed
  host key, or the app not using its private `known_hosts` is an unconditional **FAIL**,
  regardless of other results.
- If a security check cannot be run safely (for example, no disposable test server), record
  `BLOCKED` with the exact blocker. The overall result is not a pass until the owner either
  runs it or records an accepted exception with the residual risk.

### 7.2 Sign-off block

```text
Owner:                 ______________________________
Date:                  ______________________________
Platform:              macOS <version> <arch>
Overall result:        PASS / PASS WITH EXCEPTION / FAIL / BLOCKED
macOS build (M1–M7):   PASS / FAIL / BLOCKED
Functional (F1–F10):   PASS / FAIL / BLOCKED
Engine/D4 (E1–E6):     PASS / FAIL / BLOCKED
Security (S1–S6):      PASS / FAIL / BLOCKED
Linux build path:      L1 native / L2 container / not run
Windows (optional):    N/A / PASS / FAIL
Exceptions (step ID, exact blocker, evidence, residual risk):
  -
Evidence location (outside the repository):
  -
```

A `PASS` recorded before all required rows are filled is not valid.

---

## Appendix A — macOS quick run

From the repository root, after installing Node 22, Rust 1.90, and Xcode Command Line
Tools:

```sh
TARGET_TRIPLE="$(rustc -vV | sed -n 's/^host: //p')"
node scripts/fetch-engine.mjs --target "$TARGET_TRIPLE"
bash scripts/verify-engine-checksum.sh

npm ci
npm run typecheck
npm run lint
npm test
npm run build
(cd apps/spirit-desktop && npx tauri build --debug)
open target/debug/bundle/macos/StackSpirit.app
```

Then run the functional, engine, and security steps in §3–§5.

## Appendix B — Linux quick run

Native path, after installing the Tauri 2 packages in §2.3:

```sh
TARGET_TRIPLE="$(rustc -vV | sed -n 's/^host: //p')"
node scripts/fetch-engine.mjs --target "$TARGET_TRIPLE"
bash scripts/verify-engine-checksum.sh

npm ci
npm run typecheck
npm run lint
npm test
npm run build
cargo check -p spirit-desktop
cargo test -p spirit-desktop
(cd apps/spirit-desktop && npx tauri build --debug --no-bundle)
```

Container path:

```sh
scripts/desktop-build-container.sh check
```

Either path is sufficient for the Linux build requirement. The container path is the
CI-grade proof; the native path is the owner's native compile.
