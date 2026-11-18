# Handoff: StackSpirit desktop UI redesign (Meadow shell)

Read this first if you are picking up the desktop UI, the brand, or the next integration
steps. Last updated 2026-11-18. It records the decisions the owner made in the design
sessions, what is real versus preview, and what to do next. `AGENTS.md` still applies
(Rust core owns host/SSH/VOWP logic, UI calls go through `SpiritBridge`, no secrets).

## 1. Where things are

| What | Where |
|---|---|
| New desktop shell (default) | `packages/spirit-ui/src/next/` (`NextShell.tsx` is the entry) |
| Theme tokens and all shell CSS | `packages/spirit-ui/src/next/theme.css` |
| Shell unit tests | `packages/spirit-ui/src/test/next/nextShell.test.tsx` |
| Shell e2e + axe | `apps/spirit-desktop/e2e/next-shell.spec.ts` |
| Classic cockpit (still shipped) | `packages/spirit-ui/src/app/AppShell.tsx` and `views/` |
| Shell switch | `apps/spirit-desktop/src/main.tsx` (`?ui=classic` or `VITE_SPIRIT_UI=classic`) |
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

**Several servers**: independent by default; when a second server is added, suggest
making a cluster. A cluster has one **home** (source of truth for the global
orchestrator and database) and worker machines with read-only backups; a worker can be
promoted if the home dies. Matches VOWP's one-home/one-lease rule. Not built yet.

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

Preview only (clearly labelled in the UI; logic in `next/conversations.ts`):
- Orchestrator chat replies, and the agent opening the details panel.
- The Local/Server switch (label only).
- Add project (hands a message to the global orchestrator; no VOWP call).

## 5. Next steps (suggested order, updated 2026-11-18)

1. **Web app alignment** (`apps/spirit-web`) — **landed (`ss-27`, `a0619ae`).** Read-only
   Web cockpit now uses the Meadow tokens and components; classic cockpit hover contrast
   is also fixed (`ss-26`, `88be044`).
2. **One-time-password onboarding** — **core + card landed (`ss-28`–`ss-31`); small
   follow-up ready now:** auto-select and connect the saved server right after a
   successful install, and add a Settings entry point for retrying the optional
   `PasswordAuthentication no` step.
3. **Reconcile D6 local-orchestrator gate probes with VOWP `0.4.0` (ready now):** VOWP
   `0.4.0` is now shipped and installed on both the Mac and VPS (`surface rotate`, `agent
   service|doctor|trust`, `orchestrator new --node`, live `macbook` agent). Update
   `spirit-core`'s gate probes and `docs/D6-READINESS.md` / `docs/VOWP-CONTRACT.md` so
   StackSpirit recognizes VOWP `0.4.0+` cleanly.
4. **Live orchestrator chat via the paired OpenCode engine (`opencode serve`) — owner
   decision (2026-11-18):** do **not** reinvent chat or shell out to `vowp chat` per turn.
   Move `SessionId`, `SessionLocation`, `ChatMessage` and `ProjectRequest` from
   `next/conversations.ts` into `packages/spirit-ui/src/bridge/wire.ts`, and connect
   `spirit-core` / `TauriBridge` / `ChatView.tsx` to the orchestrator's OpenCode server
   (`opencode serve` locally on the Mac when running Local, or over StackSpirit's SSH
   loopback forward when running on the Server; VOWP `0.5` adds `vowp orchestrator
   endpoint` to report the active endpoint cleanly, while VOWP's bridge mirrors every turn
   to the server session log). Keep `MockBridge` scenarios working.
5. **Server secret store UI over `vowp secret` + `SECURITY.md` (ready now / pairs with
   VOWP `0.5` encrypted-at-rest vault):** wire `spirit-core` + `TauriBridge` + Settings to
   `vowp secret ls --json`, `vowp secret set <NAME>` (SSH stdin only, never argv/logs),
   and `vowp secret rm <NAME> --yes`, with the first-secret confirmation warning, stored
   keys list (`name`, `last4`, storage mode), Revoke action, and a repository `SECURITY.md`.
6. **Live `Local` / `Server` switch and `＋ Add project` (pairs with VOWP `0.5`):** once
   VOWP `0.5` lands `vowp orchestrator move` (same-session move between `--home` and
   `--node <mac>`) and one-step `vowp project add --git/--init/--orchestrator`, wire the
   session location chip/switch (defaulting to **Local** when the Mac agent is connected)
   and the sidebar **`＋` Add project** dialog to real VOWP calls.
7. **Clusters** (home + workers) after the above, then **retire the classic shell** and
   migrate its remaining e2e coverage.

## 6. Known gaps and cautions

- e2e status (updated 2026-11-18): Meadow desktop specs, web specs (`ss-27`), and classic
  cockpit WCAG contrast specs (`ss-26`, `88be044`) are all green.

- The owner has not yet seen the new icon in the Dock (`tauri dev`/`tauri build` not run
  in the design sessions).
- `design/baseline/` (frozen MockBridge builds of the old UI) is local only and ignored.
- DeepSeek Harness brand guidelines ask projects not to use the "DeepSeek Harness" name
  in their own name or promotion. We only borrowed layout patterns; keep it that way.
- Image generation for brand work used North Noir (`gpt-image-2.6-sunburst`, about $0.40
  per image). Ask the owner before generating more.
