# StackSpirit — Company Vision, Architecture & Roadmap

> **Status:** Locked-In Founder Strategy (2026-11-25)  
> **Organization:** https://github.com/Stack-Spirit  
> **Domain:** https://stackspirit.io  
> **License:** MIT — software is never the product; free open-source software drives adoption and word-of-mouth for always-on machines.  
> **Mission:** Give developers and teams an effortless, always-on machine as a home for their AI coding agents — free open-source software, monetized through affordable turnkey always-on machines (and later, hourly burst compute & home hardware).

---

## 1. Core Thesis & Positioning

AI coding harnesses (Claude Code, Codex, OpenCode, Antigravity, T3 Code, custom agents) are evolving fast. **StackSpirit does not compete by locking users into a single harness.** Instead, StackSpirit is the **always-on home and control plane underneath all of them**:

- **Always-on continuity:** Work keeps running after your laptop closes — job graphs, background verifier/fix loops, schedules, persistent worktrees, and capacity guardrails.
- **Harness-agnostic orchestration:** Orchestrate from the **StackSpirit Desktop app**, **Web App (mobile-first PWA for iOS/Android/desktop)**, **CLI (`stackspirit`)**, or directly from **any harness via MCP** (`stackspirit mcp` locally or hosted Remote MCP at `mcp.stackspirit.io`).
- **Software is free forever (MIT):** Users never pay for StackSpirit software. Bring your own VPS, spare laptop, or home server for $0.
- **Growth-first monetization:** Early profit is secondary to adoption. We sell **turnkey always-on machines** at a transparent **+20% markup** plus high-margin optional add-ons like off-site encrypted backups.

---

## 2. Brand & Packaging Rules

- **One brand everywhere:** `StackSpirit` — no "VOWP" or "StackSpirit Engine" product labels in user-facing surfaces.
- **CLI:** `stackspirit` (temporary backwards-compatible `vowp` alias/shim during migration).
- **MCP:** `stackspirit mcp` (local stdio) + hosted Remote MCP (`https://mcp.stackspirit.io`).
- **Config & State:** `stackspirit.toml` (with `vowp.toml` fallback) and `~/.stackspirit` (with `~/.vowp` compatibility/migration).
- **Package registries (accounts claimed):**
  - **npm (`stackspirit`):** Thin native binary launcher + platform optionalDependencies (`npx -y stackspirit`, `npx -y stackspirit mcp`).
  - **PyPI (`stackspirit`):** Thin wheel/launcher + optional future Python SDK (`uvx stackspirit`).
- **Legacy open-source VOWP:** `syver777/vowp` remains as the historical public VOWP release; all active development lives under `https://github.com/Stack-Spirit`.

---

## 3. Repository & Project Structure (`https://github.com/Stack-Spirit`)

**One core public MIT monorepo**, one private cloud repo for billing/provisioning/relay, and separate repos only for independent ecosystem plugins.

```text
https://github.com/Stack-Spirit
├── stackspirit              (PUBLIC / MIT — Main Monorepo, consolidated after v6 finishes)
│   ├── crates/
│   │   ├── spirit-protocol/ # Shared types, schemas, snapshot & session contracts
│   │   ├── spirit-store/    # SQLite DB, JSONL session logs, blobs, migrations
│   │   ├── spirit-graph/    # Job DAG, planner/verifier/fix state machine
│   │   ├── spirit-adapters/ # Claude Code, OpenCode, Codex, T3 Code, generic adapters
│   │   ├── spirit-core/     # Supervisor, capacity guardrails, loops, schedules, agent runtime
│   │   ├── spirit-daemon/   # HTTP/SSE/WSS home daemon + outbound relay client (+ future Iroh P2P)
│   │   ├── spirit-cli/      # `stackspirit` binary (CLI + `stackspirit mcp` + `stackspirit agent`)
│   │   └── spirit-desktop-core/ # Desktop bridge & local sidecar supervisor
│   ├── apps/
│   │   ├── desktop/         # Tauri 2 desktop cockpit
│   │   └── web/             # Responsive mobile-first PWA Web cockpit (app.stackspirit.io)
│   ├── packages/
│   │   ├── ui/              # Shared React UI (@stackspirit/ui) — Desktop + Web + Mobile PWA
│   │   ├── npm/             # `stackspirit` npm binary launcher
│   │   └── pypi/            # `stackspirit` PyPI launcher (+ future Python SDK)
│   └── docs/                # Public docs, guides, architecture, STRATEGY.md
│
├── cloud                    (PRIVATE — Cloudflare backend: accounts, relay, billing, provisioning)
│   ├── api/                 # Cloudflare Workers + D1: auth, accounts, machine registry, Stripe webhooks
│   ├── provisioning/        # Contabo API (primary), Hetzner/Fly/GCP hourly (later), abuse guards
│   ├── relay/               # Cloudflare Durable Objects: outbound WSS relay + Remote MCP bridge
│   └── backups/             # Cloudflare R2 off-site encrypted backup add-on service
│
└── Ecosystem repos (PUBLIC / MIT — Created only when needed)
    ├── asif/                # Optional standalone Agent Session Interchange Format spec
    ├── homebrew-tap/        # `brew install stack-spirit/tap/stackspirit`
    ├── omarchy-plugin/      # OS / Omarchy integrations
    └── ...                  # Community harness & OS plugins
```

**Consolidation timing:** Finish the running `v6` engine rebrand slices first, then merge `stackspirit` + `stackspirit-engine` into one `Stack-Spirit/stackspirit` monorepo (preserving git history) before flipping it public.

---

## 4. Connectivity, Accounts & Mobile/Web Architecture

### Account Model (Zero-Friction Adoption First)
1. **Account-Free Local Mode (Default, $0):**
   - Zero sign-in required. Connect any machine locally or over SSH; connection info stays strictly on the user's device (`~/.stackspirit/`).
2. **Optional Free StackSpirit Account:**
   - Needed **only** when a user wants:
     - Web App / mobile PWA access from anywhere,
     - Cross-device sign-in (connect from any laptop, phone, or MCP harness once signed in), or
     - A StackSpirit Managed Home (auto-provisioned and auto-connected).

### Transport Architecture: **Built-In MIT Outbound WSS Relay (+ Future Iroh P2P)**
- **Never store raw customer SSH private keys in the cloud.**
- **Primary cloud transport (built directly into `stackspirit`):**
  - Extends our existing outbound agent-to-home framing (`vowp-agent`) to connect outbound over authenticated TLS/WebSocket from the user's always-on machine to **`relay.stackspirit.io`** (Cloudflare Durable Objects, ~300 LOC).
  - **Zero extra daemon installs** (no required Tailscale/VPN client).
  - **Unlocks every client with one transport:**
    - **Web App / Mobile PWA (`app.stackspirit.io`):** Full orchestration from desktop browsers, iPhone, and Android (shared React UI, Add to Home Screen, Web Push) — no separate native Swift/Kotlin app needed in Stage 1.
    - **Desktop App & CLI:** Direct LAN/SSH in local mode, or relay in signed-in remote mode (including optional SSH-over-WSS via `stackspirit ssh`).
    - **Hosted Remote MCP (`mcp.stackspirit.io`):** Orchestrate from Claude.ai web/mobile, Codex, or any remote harness with zero local binary setup.
    - **NAT traversal:** Works behind home routers out of the box for Stage 4 home Mini PCs.
    - **Optional E2E encryption** between client devices and the user's machine.
- **Future P2P fast-path (Desktop/CLI ↔ Machine):**
  - Embed **Iroh** (Apache-2.0/MIT, pure Rust — uses Tailscale's exact DERP/hole-punching design) inside `stackspirit` for direct peer-to-peer transfers whenever direct UDP/QUIC is available, falling back seamlessly to `relay.stackspirit.io`.
- **BYO VPN compatibility:** Users who already run Tailscale/Headscale/WireGuard can connect directly over their private IP in account-free mode.

---

## 5. Domain, DNS & Cloud Backend (`stackspirit.io` on Cloudflare)

**DNS:** Move `stackspirit.io` nameservers from Vercel to **Cloudflare** (free, preserves existing records; any Vercel-hosted record stays grey-cloud DNS-only so Vercel SSL works cleanly, or move the marketing site to Cloudflare Pages too).

| Subdomain | Service | Role |
|---|---|---|
| `stackspirit.io` | Vercel or Cloudflare Pages | Marketing site & docs |
| `app.stackspirit.io` | Cloudflare Pages | Responsive mobile-first Web/PWA cockpit |
| `api.stackspirit.io` | Cloudflare Workers + D1 | Auth, accounts, machine registry, Stripe & Contabo API |
| `relay.stackspirit.io` | Cloudflare Durable Objects | Live outbound WSS relay & machine presence |
| `mcp.stackspirit.io` | Cloudflare Workers + Durable Objects | Hosted Remote MCP endpoint |
| `assets.stackspirit.io` / `downloads.stackspirit.io` | Cloudflare R2 | Release binaries, public assets ($0 egress) |
| `contact@stackspirit.io` / `support@stackspirit.io` | Cloudflare Email Routing | Free forwarding |

**Baseline backend cost:** ~$5/month flat (Cloudflare Workers Paid covers Workers, D1, Durable Objects, R2 free tier, Pages).

---

## 6. Business Model, Billing & Compute Roadmap

### Payments & Tax
- **Processor:** **Stripe + Stripe Tax** (familiar stack, lowest card fees; founder handles Norwegian MVA / EU VAT directly).

### Pricing Philosophy: **Adoption First (+20% Markup + Optional High-Margin R2 Backups)**
- **Software:** Free & MIT open-source forever.
- **Managed Always-On Homes (Primary Product — Contabo):**
  - **+20% markup** on Contabo's **regular (non-promo) excl.-VAT monthly price** across all tiers starting from **Cloud VPS 4 (8 GB RAM)** so users can enter as cheaply as possible.
  - **Setup fee handling:** On tiers where Contabo charges a one-time setup fee (e.g., Cloud VPS 4), pass through the setup fee with the same **+20% markup** as a one-time Stripe Checkout line item. Tiers with "No Setup Fee" (Cloud VPS 6+) have €0 setup fee.

| StackSpirit Tier | Underlying Contabo Spec | Contabo Regular (excl. VAT) | Setup Fee | Target StackSpirit Price (+20% on regular) |
|---|---|---:|---|---:|
| **Home 8** | Cloud VPS 4 — 4 vCPU, 8 GB RAM, 100 GB SSD | €5.50/mo | Contabo setup fee + 20% (one-time) | **~€6.60–€7/mo** |
| **Home 12** | Cloud VPS 6 — 6 vCPU, 12 GB RAM, 200 GB SSD / 100 GB NVMe | €7.50/mo | €0 | **€9/mo** |
| **Home 24** | Cloud VPS 8 — 8 vCPU, 24 GB RAM, 300 GB SSD / 150 GB NVMe | €14.00/mo | €0 | **€17/mo** |
| **Home 48** | Cloud VPS 12 — 12 vCPU, 48 GB RAM, 400 GB SSD / 200 GB NVMe | €25.00/mo | €0 | **€30/mo** |
| **Home 64** | Cloud VPS 16 — 16 vCPU, 64 GB RAM, 500 GB SSD / 250 GB NVMe | €37.00/mo | €0 | **€44/mo** |

- **Optional Add-On — StackSpirit Off-Site Encrypted Backups (Cloudflare R2):**
  - **Off by default, strongly recommended** during checkout and in machine settings.
  - Contabo VMs are provisioned **without** Contabo Auto Backup.
  - StackSpirit backs up `~/.stackspirit` DB, sessions, configs, and selected repo state encrypted to **Cloudflare R2** ($0.015/GB/mo, $0 egress), priced around Contabo's backup tier (~€2–€5/mo) → **~80–90% gross margin** AND superior reliability (off-provider, portable across VPS providers and future home Mini PCs).

### Day-1 Abuse Safeguards (Mandatory)
- **No unpaid free trials** on managed VMs (verified payment card required upfront).
- **Block outbound SMTP (ports 25, 465, 587)** via firewall/cloud-init by default.
- **Per-customer VM isolation** (unique credentials, isolated firewall rules, automated instance/spend limits).
- **Honest provisioning UX:** Contabo API provisioning takes a few minutes via `cloud-init`; show live provisioning progress and notify via email/Web Push the moment `relay.stackspirit.io` sees the Home online.

### Phased Roadmap
| Stage | Focus | Highlights |
|---|---|---|
| **Stage 1 (Now)** | **Open-Source Foundation** | Finish `v6` rename → consolidate into public MIT `Stack-Spirit/stackspirit` monorepo → publish `stackspirit` on npm & PyPI → rock-solid free BYO-machine experience (CLI + MCP + Desktop + Web). |
| **Stage 2 (First Revenue)** | **Managed Monthly Homes + Cloud Sync** | Move `stackspirit.io` DNS to Cloudflare → build private `Stack-Spirit/cloud` (Workers/D1/Durable Objects/Pages/R2) → account-free default + optional free account (`relay.stackspirit.io` + `app.stackspirit.io` PWA + `mcp.stackspirit.io`) → 1-click **Contabo** monthly homes (+20%, incl. VPS 4 with +20% setup fee) + **R2 Backup** add-on (off by default, strongly recommended) + Stripe. |
| **Stage 3 (Hourly / Burst Compute)** | **Try-It Hourly & Burst Workers** | Add **Hetzner Cloud**, **Fly.io Machines**, or **Google Cloud** hourly/per-second compute so users can test cheaply by the hour or burst heavy agent/build loads on demand. |
| **Stage 4 (Built-In Agents & Home Mini PCs)** | **Hardware + Native Agents** | Optional built-in multi-model VPS agents; pre-configured **StackSpirit OS image** + dropshipped/partner **Home Mini PCs** using the exact same outbound WSS relay + QR pairing. |
| **Stage 5 (Owned Infrastructure)** | **Vertical Integration** | Custom StackSpirit Mini PCs and owned colocated bare-metal server racks once recurring wholesale compute scale justifies capex. |

---

## 7. Immediate Operational Next Steps (Stage 1 → Stage 2 Bridge)

1. **Finish `v6` engine rebrand** on the VPS, then merge `stackspirit` + `stackspirit-engine` into the single `Stack-Spirit/stackspirit` MIT monorepo.
2. **Move `stackspirit.io` DNS to Cloudflare** (preserve existing Vercel records as DNS-only grey cloud, or host static site on Cloudflare Pages too).
3. **Verify Contabo API & Reseller setup:** Enable Contabo API credentials in Customer Control Panel, fetch exact `productId` catalog + VPS 4 setup fee, check provisioning latency, and inquire about Contabo's Reseller/Partner program for wholesale discounts & higher instance quotas.
4. **Sign in to Cloudflare (`wrangler`), npm, and PyPI** when ready to publish packages and scaffold `Stack-Spirit/cloud`.
