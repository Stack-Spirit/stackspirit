# Handoff: StackSpirit desktop UI redesign (Meadow shell)

Read this first if you are picking up the desktop UI, the brand, or the next integration
steps. Last updated 2026-11-19. It records the decisions the owner made in the design
sessions, what is real versus preview, and what to do next. `AGENTS.md` still applies
(Rust core owns host/SSH/VOWP logic, UI calls go through `SpiritBridge`, no secrets).

## 1. Where things are

| What | Where |
|---|---|
| Desktop shell | `packages/spirit-ui/src/next/NextShell.tsx` is the only shell; the classic frame and its classic-shell switch were retired (`ss-52`, `aba207d`) |
| Theme tokens and all shell CSS | `packages/spirit-ui/src/next/theme.css` |
| Shell unit tests | `packages/spirit-ui/src/test/next/nextShell.test.tsx` |
| Shell e2e + axe | `apps/spirit-desktop/e2e/next-shell.spec.ts` |
| Brand sheet, logo sources, icon builders | `design/brand/` (see its `README.md`) |
| App icons | `apps/spirit-desktop/src-tauri/icons/` (built by `design/brand/tools/app_icon.py`) |
| Preview servers for Claude Code | `.claude/launch.json` (`desktop-mock`, `brand`, `baseline`) |

## 2. Run it

```sh
npm ci
VITE_SPIRIT_MOCK=1 npm run dev -w @stackspirit/desktop   # http://127.0.0.1:1420
```

- Preview data: `?scenario=empty` (no server), `?scenario=busy` (saved server; click
  Connect), any name in `MOCK_SCENARIOS` (`packages/spirit-ui/src/bridge/MockBridge.ts`).
- Theme: follows the system; `&theme=light|dark` forces one; Settings → Theme too.
- Brand sheet: `python3 -m http.server 4174 --directory design/brand`, then open
  `spirit-logos.html` and `mark.html`.
- Gates: `npm run typecheck`, `npm run lint`, `npm test`, `npm run test:e2e`
  (needs `npx playwright install chromium` once), `cargo check -p spirit-desktop`.

## 3. Owner decisions (do not re-litigate without asking)

**Brand**
- Palette **A · Meadow**: primary `#43A85A`, deep `#1E5B36`, sprout `#CDEDB0`, light
  `#F5F7F0`, dark `#0D120F`. Text and buttons use darker tokens to pass WCAG AA (buttons
  `#2E7F48` in light mode); `#43A85A` stays for the mark, bars and dots.
- Logo: the deadpan green spirit face with frog eyes and a two-leaf sprout. The logo file
  is `design/brand/mark/stackspirit-mark-master.png` (the generated image with colours
  snapped to 8; matches the source exactly). 16/32 px icons are hand-drawn.
- Thicker full body and the deadpan "Gazer" are mascots, not logos.
- Product is meant to become a professional service (setting up AI frameworks on
  always-on machines and clusters) and eventually open source under MIT.

**Layout (modelled on DeepSeek Harness; layout only, never its name or assets)**
- Left sidebar: logo row, server card (one **Load** number = the higher of RAM and CPU,
  active workers, free disk; Connect/⋯ menu here), Global orchestrator, Projects with
  ＋ (each project opens/closes and contains **Orchestrator** and **Workers**), one
  **Settings** line at the bottom with a 3-item menu (Add server, Theme, All settings).
  32 px rows, 14 px text, list fades under the footer instead of a divider. Width is
  draggable and remembered.
- Main view: an orchestrator chat (DeepSeek-style composer), the **Server** page (health
  cards, node map, project rows, capacity controls), a project's **Workers** page (node
  map orchestrator → lanes → jobs, Automation-style rows with search and status filter,
  classic lane board as "Board"), or Settings.
- Right **details panel**: hidden by default; opens from the header toggle, when a node
  or row is selected, or when an agent reply points at something. Draggable width.
- No saved server → full-page welcome with **Add server** and **Set up with AI**.
  Saved but offline → one Connect action.

**Naming**: "Server" for the always-on machine (VPS or a computer at home reached over
SSH), "Local" for this Mac. Not VM/AOM.

**Sessions**: one global orchestrator plus one orchestrator session per project. Each
session shows where it runs with a chip (green **Server · name**, blue **Local · this
Mac**) and a one-click switch. The session log always lives on the server. Today
sessions run on the server; the owner wants **Local by default later**, once VOWP's
local orchestrator works, so agents get local-machine context.
- Server chat transport is decision **A** (2026-11-19): wait for VOWP `0.5`'s
  `vowp orchestrator endpoint`; the app-owned SSH forward is **not approved**; Local chat
  on the bundled engine is the working path. Revisit only if the owner asks.

**Several servers (Pooled-Compute + Failover Clusters)**: independent by default (sidebar
lets you switch between saved servers). When a second server is added, offer **"Form a
cluster"**:
- Pick **1 Home server** (source of truth for the global orchestrator, databases and
  session logs) and **1+ Worker/Backup servers**.
- **Combined cluster compute (not passive standby):** Worker servers are enrolled as
  active VOWP agent nodes (`vowp node enroll` + `vowp agent enroll` + `vowp agent service
  install --now`) with `[offload] enabled = true`, `default_placement = "any"`, and
  `result_transport = "bundle"` so ready worker jobs spread across **both** the Home and
  all Worker servers simultaneously. Two identical 4-worker (or 8-worker) servers run up
  to **8 (or 16) concurrent workers**. The owner decision expected VOWP `0.5` Slice 6
  pooled cluster capacity to back this; `0.5` is not installed yet, so pooled numbers are
  computed from Home and the online workers (see §5 step 7). The sidebar Server card and
  Server page display combined cluster Load, combined active/max workers (`home +
  workers`), and per-server health/node map.
- **Hot backup & Home promotion:** Home also runs scheduled `vowp backup` snapshots to the
  Worker/Backup server(s), and StackSpirit provides an explicit, confirmation-gated
  **"Promote backup to Home"** action (`vowp backup --restore` + `vowp up` + repointing
  active Home). It requires the old Home to answer `vowp down --drain` before anything is
  restored, so a completely unreachable Home is not yet supported (see §5 step 7).
  Matches VOWP's one-home/one-lease rule.

**SSH and secrets** (security model the owner approved)
- Keep the existing OpenSSH model in `crates/spirit-core/src/ssh` (system `ssh`,
  `BatchMode`, `StrictHostKeyChecking`, private `known_hosts`, fingerprint pin,
  `IdentitiesOnly`, profiles store only a key *path*).
- Add: in-app ed25519 key creation (passphrase in the macOS Keychain) and agent support.
- New servers: a **one-time password** typed into an app-owned secure field; the app logs
  in once, installs a fresh key, optionally turns off password login, and forgets the
  password. Never saved, never shown to the AI.
- Provider keys **may** now live on the server (this replaces the old "keys never leave
  the Mac" rule), **reusing VOWP's `vowp secret` vault (`set` over verified SSH stdin,
  `ls --json` returning only `name` + `last4`, `rm --yes`, `upgrade` / v0.5 encrypted-at-rest
  vault)** — never invent a second custom secret store in StackSpirit. Keys are sent only
  over the verified SSH connection on **stdin** (never argv or logs), never visible to the
  AI (which only sees names / "stored ✓"), and automatically `[REDACTED:<NAME>]`-scrubbed by
  VOWP. Show a warning the user confirms before the first secret is stored, plus a list
  with Revoke.
- Legal/positioning: MIT "as is" covers liability; promise *design* in a `SECURITY.md`,
  not liability. A paid service needs its own terms (lawyer).

**AI setup**: use the bundled OpenCode sidecar ("StackSpirit Engine", already shipped in
D4) and its free included models for the setup assistant, so it works with no provider
or key. Free model availability is OpenCode's, so the UI must stay honest about it.

## 4. Real versus preview

Real (store + bridge, works against a server): hosts, add-server wizard, connect and
disconnect, fleet, capacity, inbox (answers via the classic inbox drawer), jobs and the
lane board actions, capacity controls, sessions/.asif, raw document, engine and gates
(Settings → Advanced, including the D6 local-orchestrator panel), errors and retry, and
the setup assistant's one-time-password onboarding. That card scans the server's host
keys, shows every `SHA256:` fingerprint and requires an explicit "I verified this
fingerprint" confirmation; it then creates the app-owned ed25519 key (passphrase in the
OS keychain) and installs it through the `onboarding` bridge (`onboarding.createKey` +
`onboarding.installKey`, backed by `spirit_core::ssh::keygen` and
`spirit_core::ssh::password_install`). The optional `PasswordAuthentication no` step is
reported honestly (`disabled`, `not_supported` with its reason, `failed` with its
detail, or `not_requested`), and the one-time password is cleared before the install
call, never rendered, logged, or stored. Only a verified key-only login is success.
After a verified install the shell refreshes the hosts store, selects the saved
`HostProfile`, and connects it automatically; a failed automatic connection stays in
the error bar and the profile is still saved and retryable. Settings → **Password
login** repeats the optional `PasswordAuthentication no` step for a saved server whose
profile uses the app key. That retry also asks for a one-time server password because
the current `spirit_core` install API needs one more password login to re-check the key
and apply the sshd drop-in; it is cleared before the call and reports `disabled`,
`not_supported` or `failed` exactly.

The cluster path is implemented end to end on the desktop host (`ss-54`, `fe3146b`;
`ss-55`, `58a37f2`, `04c9fdd`). `crates/spirit-core/src/cluster/mod.rs` owns the typed
model and every VOWP command; `apps/spirit-desktop/src-tauri/src/commands/cluster.rs`
adapts the five Tauri commands (`cluster_form`, `cluster_health`, `cluster_promote`,
`cluster_backup_policy`, `cluster_remove_worker`) to the optional
`SpiritBridge.cluster` namespace (`form`, `health`, `promote`, `backupPolicy`,
`removeWorker`). Meadow renders it through
`packages/spirit-ui/src/next/FormClusterDialog.tsx`, `ClusterSection.tsx` and
`PromoteBackupDialog.tsx`, with pure helpers in
`packages/spirit-ui/src/next/cluster.ts`. Cluster state lives in
`packages/spirit-ui/src/next/clusterStore.ts`, beside `fleetStore` rather than inside it:
the store reads the bridge and host list from `fleetStore`, keeps a failed health read
local (dropping the stale pooled total) instead of dimming the fleet view, and skips the
read entirely in preview. Preview mode therefore never shows a cluster or a pooled
number: every `cluster` action returns typed `preview_unavailable`.

Settings → Advanced → **Server secrets** lists, stores and revokes keys in VOWP's own
vault (`ss-38`, `bf45d31`; `ss-34`, `def3538`; `ss-35`, `506d9d2`). A value is
write-only: it leaves the app only over the verified SSH connection on stdin, after
VOWP's hidden `value for <NAME>: ` prompt, and never through argv, the environment, a
log or an error. The card shows `name`, `•••• last4`, the update time and a storage mode
when VOWP reports one; VOWP `0.4.0` reports none, so the card says so instead of
guessing. See [`SECURITY.md`](../SECURITY.md) for the exact path and limits.

Chat has three surfaces (see §5 step 4): **setup chat** on the bundled engine (below);
**Local `global`/`project`** through `vowp agent orchestrator endpoint`, real when the
installed VOWP reports a local orchestrator and `chat_not_configured` otherwise
(endpoint-verified on the owner's Mac; not live on this Linux host's VOWP `0.4.0`, which
has no local orchestrator record); and **Server `global`/`project`**, gated (`chat_gated`,
decision A). The shell opens `global`/`project` on Local (`a62c6e8`); Server remains the
gated path.

The setup assistant's chat is real on the bundled StackSpirit Engine (`ss-42`, `cc83371`;
`ss-45`, `fe6d6e5`; `ss-46`, `c1e2402`). It creates or resumes its private local session,
sends one turn through `bridge.chat`, and renders the real final model reply; it never
starts a second engine and never invents an assistant reply. **Live token streaming is
disabled for now:** the production transport reports `streaming_supported: false` because
the pinned v1 event names and schema are recorded but the runtime cadence and ordering of
`message.part.updated`, `message.part.delta` and `session.idle` are unproven without an
owner model turn (`docs/ORCHESTRATOR-CHAT.md` §2.2.5 and §9.1; `ss-43` live probe;
verification script `ss-47`, `76b48ac`). The **non-streaming final message is real**: the
reply arrives as one complete assistant message, not token by token. Do not call it
streaming until the owner verification records the cadence.

Preview only (clearly labelled in the UI; logic in `next/conversations.ts`):
- Assistant replies in the `MockBridge` preview scenarios, and the agent opening the
  details panel. Every preview assistant message starts with `Preview reply.` and carries
  `origin: "preview"`; a real bridge drops that origin instead of rendering it (`ss-46`,
  `c1e2402`).
- Add project (hands a message to the global orchestrator; no VOWP call).

Gated, not preview: Server `global`/`project` chat shows a typed gate (`chat_gated`)
with the composer disabled and no assistant reply. The blocker is VOWP `0.5`'s
`vowp orchestrator endpoint` (`docs/ORCHESTRATOR-CHAT.md` §3.6 and §7.2); the app-owned
SSH forward is not approved (owner decision A, 2026-11-19). Local `global`/`project` is
not gated when the installed VOWP reports an endpoint: it attaches there, continues the
newest host session when no native id is reported, and never creates a session or falls
back to the bundled engine (`ss-61`, `66563cd`; `ss-62`, `0237770`, `147e153`). The
Local/Server location is a read-only chip derived from the transport; there is no live
switch. Moving a session is `vowp orchestrator move` (VOWP `0.5`, §5 step 6), so it stays
pending too.

## 5. Next steps (suggested order, updated 2026-11-19)

1. **Web app alignment** (`apps/spirit-web`) — **landed (`ss-27`, `a0619ae`).** Read-only
   Web cockpit now uses the Meadow tokens and components; classic cockpit hover contrast
   is also fixed (`ss-26`, `88be044`).
2. **One-time-password onboarding** — **landed (`ss-28`–`ss-31`, `ss-33`; auto-connect
   and Settings retry `44356bb`, follow-up `8768daf`).** The secure card auto-selects
   and connects the saved server right after a verified install (a connection error is
   shown honestly and keeps the saved profile), and Settings → **Password login**
   retries the optional `PasswordAuthentication no` step on a server whose profile uses
   the app key, reporting `disabled`, `not_supported` or `failed`.
3. **Reconcile D6 local-orchestrator gate probes with VOWP `0.4.0`** — **landed
   (`ss-32`, `cd6a975`, follow-up `20181a5`; live home evidence `ss-37`, `39d8538`).**
   VOWP `0.4.0` is shipped and installed, and the gate probes now match its `surface
   rotate`, `surface pair --replace`, `agent service|doctor|trust`, `orchestrator new
   --node` and `mcp --agent-socket` surface; `docs/D6-READINESS.md` and
   `docs/VOWP-CONTRACT.md` cite that verified output. The live home read on 2026-11-18
   records the `macbook` card as `kind: agent`, `status: active`, `presence: online`,
   protocol `vowp-agent/1`; the home node is `kind: home` and offline. The Mac-side
   `vowp agent status` (`connection: connected`) is still an owner check, the live agent
   reports `agent_version: 0.3.0`, and the gate therefore stays `Waiting` with D6
   actions disabled.
4. **Live orchestrator chat via the paired OpenCode engine (`opencode serve`) — three
   surfaces (`ss-39`, `8980bd9`; `ss-40`, `07dbd2d`; probe `ss-41`, `4785839`; live probe
   `ss-43`; facts `ss-44`, `ff89ea6`; setup core `ss-42`, `cc83371`; host/bridge `ss-45`,
   `fe6d6e5`; view `ss-46`, `c1e2402`; owner verification script `ss-47`, `76b48ac`;
   Local endpoint `ss-61`, `66563cd`; newest-session continuity `ss-62`, `0237770`,
   `147e153`; live verification `ss-63`, `1528aba`).** The owner decision stands: use the
   orchestrator's OpenCode server, not a new chat store or a per-turn `vowp chat`
   shell-out.
   - **Setup chat** — real on the bundled StackSpirit Engine through `bridge.chat`: it
     creates or resumes its private local session, sends one turn, and renders the real
     final reply. It never spawns a second engine and never shows a preview reply when
     `bridge.preview` is false. It is unchanged, and remains non-streaming for now (see
     the caveat in §6).
   - **Local `global`/`project`** — landed and version-gated. It attaches when the
     installed VOWP provides `vowp agent orchestrator endpoint <name> --json`;
     `crates/spirit-core/src/engine/chat/endpoint.rs` runs it with argv only, a bounded
     timeout and typed refusals (`chat_not_configured`, `chat_gated`). When the endpoint
     reports `native_session_id: null`, the app continues the newest host session
     (`GET /api/session`, greatest `time.updated`); it never creates a native session and
     never falls back to the bundled engine. The owner's Mac endpoint document was
     live-verified 2026-11-19 (`app: opencode`, loopback URL, Basic auth,
     `native_session_id: null`; no id, URL or port is recorded here). This Linux host runs
     VOWP `0.4.0` with no local orchestrator record, so it honestly returns
     `chat_not_configured`; Local chat is not live here. The §9.1b owner run in the
     checklist is still outstanding.
   - **Server `global`/`project`** — still gated. VOWP `0.4.0` has no
     `vowp orchestrator endpoint`, so Server chat returns the typed `chat_gated` gate
     until VOWP `0.5` ships the endpoint. The owner chose that endpoint (decision **A**,
     recorded with `ss-50` on 2026-11-19) and did **not** approve the app-owned SSH
     forward; the `chat_gated` behavior is unchanged. The **Local/Server switch** still
     needs VOWP `0.5` `vowp orchestrator move` (step 6); the location chip stays
     read-only.
   The contract is [`docs/ORCHESTRATOR-CHAT.md`](ORCHESTRATOR-CHAT.md) §2–§7 and
   §9; the owner checklist is
   [`scripts/verify-orchestrator-chat.mjs`](../scripts/verify-orchestrator-chat.mjs), and
   `node scripts/verify-orchestrator-chat.mjs --check` passes (static preconditions; the
   §9.1b owner run is outstanding).
5. **Server secret store over `vowp secret` + `SECURITY.md`** — **landed (`ss-38`,
   `bf45d31`; `ss-34`, `def3538`; `ss-35`, `506d9d2`).** Settings → Advanced → **Server
   secrets** reads `vowp --json secret ls`, stores with `vowp --json secret set <NAME>`
   over prompt-gated SSH stdin, and revokes with `vowp --json secret rm <NAME> --yes`.
   It has the first-use warning, the stored-key list (`name`, `•••• last4`, update time,
   storage mode when reported) and an in-card Revoke. The `set` path shipped; it is not
   a `not_supported` fallback. The value is written only after VOWP's exact hidden
   `value for <NAME>: ` prompt and never appears in argv, the environment, a log or an
   error. VOWP `0.4.0` does not report whether the vault is basic or vault-user mode,
   so the card says that rather than guessing; VOWP `0.5` is the planned
   encrypted-at-rest upgrade. [`SECURITY.md`](../SECURITY.md) records the exact path and
   limits.
6. **Live `Local` / `Server` switch, `＋ Add project`, and live D6 flip (pairs with VOWP
   `0.5`) — still pending:** VOWP `0.5` `vowp orchestrator move` (`b85a8de` / `b7a50f8`)
   and encrypted `vowp secret` (`c601bc6`) are merged, with
   `project add --git/--init/--orchestrator` and `orchestrator endpoint` landing next, and
   the Mac agent is now running `vowp 0.4.0` (`connected`). Flip D6 readiness probes on
   the live `0.4.0+` agent, wire the session location chip/switch to
   `vowp orchestrator move` (defaulting to **Local** when the Mac agent is connected), wire
   the sidebar **`＋` Add project** dialog to VOWP's one-step project add + orchestrator
   creation, and surface the VOWP `0.5` vault storage mode badge (`basic` / `vault_user` /
   `encrypted`) in Settings → Server secrets. The local setup chat landing did not change
   this order: the chat location chip is read-only, and no switch is offered until these
   commands land.
7. **Pooled-Compute & Failover Clusters (owner decision 2026-11-18 — Full VOWP-Native
   Cluster V1) — landed in two halves (`ss-54`, `fe3146b`; `ss-55`, `58a37f2`,
   `04c9fdd`).** Multiple saved servers stay **independent by default** with the existing
   sidebar switcher; **Form a cluster** appears only when 2+ servers are saved. The core
   (`crates/spirit-core/src/cluster/mod.rs`) models `ClusterConfig`, `ClusterWorker`,
   `BackupPolicy` and `ClusterHealth`, enrolls each worker over verified SSH (`vowp node
   enroll`, `vowp agent enroll --code-file -`, `vowp agent service install --now`,
   `vowp agent paths add`), enables pooled compute in `[offload]`, schedules `vowp backup`,
   and implements the confirmation-gated promotion that refuses a stale backup. The UI
   half is the `SpiritBridge.cluster` namespace plus the Tauri commands in
   `apps/spirit-desktop/src-tauri/src/commands/cluster.rs` (`form`, `health`, `promote`,
   `backupPolicy`, `removeWorker`), Meadow's `FormClusterDialog`, `ClusterSection` and
   `PromoteBackupDialog`, `packages/spirit-ui/src/next/cluster.ts`, the sidebar pooled
   **Load** / `cluster` badge, and the Server page cluster section; cluster state is read
   through `packages/spirit-ui/src/next/clusterStore.ts` (see §4). The sidebar card
   shows pooled Load and `busy / pooled target` workers; the Server page lists Home and
   each worker with its state and slots, the saved backup policy, and the promote action.
   A completed promotion shows the restored Home identity and offers an explicit
   **Connect to** action; it never switches the active server silently.
   - **Honest limits:** `ClusterHealth` has no last-backup timestamp, so the UI shows the
     policy's maximum age at promotion (`freshness ≤ every`) and never invents a "last
     backup" time. The sidebar **Load** in a cluster is pooled worker utilization
     (`busy / pooled target`), not the single-host RAM/CPU figure. Promotion needs the old
     Home to answer `vowp down --drain`, so it cannot promote while Home is unreachable.
     VOWP `0.5`'s additive `cluster` capacity totals are not installed; the core computes
     the numbers from Home and the visible online workers, labels them `computed`, and the
     UI says so. Mock-mode e2e can only prove the honest `preview_unavailable` gate; a
     real cluster needs the desktop host.
8. **Full Meadow Cutover — retire the classic shell & complete Meadow E2E (owner decision
   2026-11-18) — landed (`ss-52`, `aba207d`; `ss-53`, `6f2a568`, `9d7d357`, `dde41d9`;
   `ss-56`, `a80775e`).** Meadow (`NextShell.tsx`) is the only shell. The classic frame
   (`AppShell.tsx`, `TopBar.tsx`, `StatusStrip.tsx`, `Drawers.tsx`), its tests, the
   `AppShell`/`TopBar`/`StatusStrip` exports, the classic-shell switch, and the unused
   classic CSS were removed; `scenarios.test.tsx` now drives Meadow's `EngineView` and
   `GateCard`. The shared operational drawers/views Meadow still uses (`AddHostWizard`,
   `InboxDrawer`, `SessionsDrawer`, `RawDocumentDrawer`, and the Workers `Board` lane view)
   stay under Meadow tokens. Desktop Playwright now runs on the default Meadow shell:
   `playwright.config.ts` has no classic env and no `testIgnore`, and the stale classic
   `apps/spirit-desktop/src/test/e2e/fleet.spec.ts` was deleted rather than skipped. `ss-53`
   also fixed two WCAG AA contrast misses in `packages/spirit-ui/src/next/theme.css`
   (`.badge--accent` and `.btn--danger:hover`) and added coverage for onboarding
   password-login, the Server secrets card, the read-only location chip and the `chat_gated`
   composer. Proven green: `ss-56` (`a80775e`) ran
   `npm run test:e2e -w @stackspirit/desktop` at **18/18 on two consecutive runs** (2.6 m
   and 1.9 m), with zero failed, skipped or flaky tests, no waivers, and no spec or config
   change. Step 6 (Local/Server switch, live Server chat, Add project) remains pending and
   is not claimed here; step 7 landed as `ss-54`/`ss-55` above.

### 5.1 Plan after the first live run (2026-11-19)

Running the app against the real Mac orchestrator found problems that fake-backed tests missed
(the shell still asked for a **Server** location although only **Local** chat is live). Next, in
order, with the VOWP side tracked in `vowp-private` `docs/design/v0.5.1/scope.md`:

1. **Set up with AI respects an existing profile.** With a saved server, the setup assistant opens
   directly as a normal chat on the bundled engine's free models, without requiring Connect and
   without the new-server password form. A saved server may be shown as an info card.
2. **Server chat transport options** (document only): what a home-hosted orchestrator's endpoint
   allows, versus an app-owned SSH forward, versus keeping Server chat gated. The owner decides.
3. **Session ownership in the UI** (after VOWP v0.5.1 slice 3): show which surface holds the
   session (OpenCode, StackSpirit, CLI), take over explicitly, and make the previous writer
   read-only. Until then do not type into the same orchestrator from two windows.
4. **Use the VOWP-reported native session id** (after VOWP v0.5.1 slice 2) and drop the
   "newest host session" heuristic.
5. **Owner style advice** to be turned into a UI job when supplied.
6. **Live owner smoke test** stays an owner step: the real endpoint lives on the owner's Mac, so a
   server-side worker cannot exercise it.

## 6. Known gaps and cautions

- e2e status (updated 2026-11-19): the Meadow desktop suite passed 18/18 on two
  consecutive runs (`ss-56`, `a80775e`; 2.6 m and 1.9 m, zero failed, skipped or flaky,
  no waivers and no spec or config change). The specs are `next-shell`, `meadow-flows`,
  `meadow-bridge-flows`, `sessions-asif`, `local-orchestrator-gated` and
  `local-orchestrator-ready`.
- **Cluster product gaps.** `ClusterHealth` carries no last-backup timestamp, so freshness
  is shown as the backup policy's maximum age at promotion, not an observed backup time.
  Promotion requires the old Home to answer `vowp down --drain`, so it cannot rescue an
  unreachable Home. VOWP `0.5`'s additive `cluster` totals are not installed; pooled
  numbers are computed from Home and the visible online workers and labelled `computed`.
- **Setup chat is non-streaming for now.** The production transport reports
  `streaming_supported: false`: the pinned v1 event names and schema are recorded, but the
  runtime cadence and ordering of `message.part.updated`, `message.part.delta` and
  `session.idle` are unproven without a model turn, so the final assistant message is
  rendered as one complete message. The reply is real; do not describe it as token-by-token
  until the owner verification records the cadence (`docs/ORCHESTRATOR-CHAT.md` §2.2.5,
  §9.1). The incremental text is absent by design, not a failed render.
- **Pinned sidecar and probe.** The bundled pin is `opencode` `1.19.5`
  (`engine/manifest.json`). The hermetic probe script (`ss-41`, `4785839`) and the live
  pinned-engine probe (`ss-43`; facts applied to the contract in `ss-44`, `ff89ea6`) pin
  the v1 health/session/message/abort/SSE surface. The owner-run check and checklist are
  `scripts/verify-orchestrator-chat.mjs` (`ss-47`, `76b48ac`); `--check` fails closed when
  the sidecar or lock is absent (a fresh checkout must run
  `node scripts/fetch-engine.mjs` first), and `--checklist` prints the §9 steps. No owner
  model turn has been recorded, so the streaming cadence and the Server-mirroring half
  remain unverified.
- The Password login and Server secrets cards now have Playwright coverage in
  `meadow-bridge-flows.spec.ts` (`ss-53`, `6f2a568`), including the three password-login
  outcomes, masked `last4`, Revoke, the read-only location chip and the `chat_gated`
  composer, with axe scans in light and dark.
- VOWP `0.4.0` does not report the vault storage mode and does not encrypt the vault at
  rest. The Server secrets card shows the mode as not reported, and
  [`SECURITY.md`](../SECURITY.md) records the exact prompt-gated set path and limits;
  the app never runs `vowp secret upgrade` on its own.
- D6 actions remain disabled. The live home read (`ss-37`, `39d8538`) records an online
  `macbook` agent card with protocol `vowp-agent/1`, but the Mac-side
  `vowp agent status` is unverified and the live agent reports `agent_version: 0.3.0`;
  the home node is offline. The gate stays `Waiting`, D6 actions are not enabled, and no
  live desktop smoke has been recorded.

- The owner has not yet seen the new icon in the Dock (`tauri dev`/`tauri build` not run
  in the design sessions).
- `design/baseline/` (frozen MockBridge builds of the old UI) is local only and ignored.
- DeepSeek Harness brand guidelines ask projects not to use the "DeepSeek Harness" name
  in their own name or promotion. We only borrowed layout patterns; keep it that way.
- Image generation for brand work used North Noir (`gpt-image-2.6-sunburst`, about $0.40
  per image). Ask the owner before generating more.
