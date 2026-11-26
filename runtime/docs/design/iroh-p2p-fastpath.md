# Iroh P2P fast-path (design)

**Status: design only, not shipped.** Nothing in this document is live in the current
`stackspirit`/`vowp` binaries. The existing transport remains the authenticated WebSocket relay at
`wss://relay.stackspirit.io`, and every command keeps working without Iroh. The design is recorded
here so the desktop, CLI and machine-side work can be split into small, gated jobs.

Related documents:

- [`relay-and-packaging.md`](relay-and-packaging.md) — the cloud API, relay tickets and the
  end-to-end channel that this fast path reuses as its fallback.
- [`docs/HANDOFF-DESKTOP-UI.md`](../../../docs/HANDOFF-DESKTOP-UI.md) — the desktop surfaces
  and the account-free local/SSH default.
- [`stackspirit-engine-rebrand.md`](stackspirit-engine-rebrand.md) — `stackspirit` is primary and
  `vowp` remains a compatibility command with the same command tree.

## 1. Goal

Add an optional direct QUIC path between two StackSpirit peers:

- Desktop or CLI ↔ an Always-On Home / machine.
- Desktop ↔ Desktop for a future shared-session surface.
- Machine ↔ machine for clustered workers and backup promotion.

The fast path must be **invisible when it works and harmless when it does not**. Every transfer
first tries a direct Iroh connection with a short deadline; when that fails it continues through the
existing `wss://relay.stackspirit.io` channel with the same application framing. Users do not choose
a transport for each command, and no command gains a new failure mode merely because a direct path
was unavailable.

Non-goals for the first slice:

- No replacement of the relay. The relay stays the authenticated control plane and the universal
  fallback.
- No new account, key or secret store. Iroh identities are derived from, or bound to, the existing
  machine Ed25519 identity.
- No payload visibility for `api.stackspirit.io`, Cloudflare or the relay.
- No default-on UDP ingress. `off` is the default until the owner enables it.

## 2. Why Iroh

[Iroh](https://iroh.computer) is a Rust QUIC toolkit with:

- QUIC transport (TLS 1.3, multiplexed streams, congestion control) over UDP.
- Hole punching for direct connections and an encrypted relay/DERP path when a direct path is
  impossible.
- Ed25519 node identities and signed node tickets.
- Content-addressed blobs (`iroh-blobs`) for resumable transfers.
- Dual **Apache-2.0 / MIT** licensing, compatible with this repository's MIT distribution.

The alternative — writing our own UDP hole punching, NAT traversal and congestion control — is a
large, security-sensitive subsystem that is not differentiating for StackSpirit. Iroh is the
smallest dependency that provides the same properties.

**Version risk.** Iroh is still pre-1.0. The dependency must be pinned to an exact minor version,
behind an off-by-default Cargo feature, and isolated behind a small `P2pTransport` trait so a
breaking upgrade does not reach command handlers. A machine that does not enable the feature never
links the crate.

## 3. Where the fast path sits

```text
Desktop / CLI                         api.stackspirit.io                 Home / machine
     │                                       │                                  │
     │ 1. POST /v1/relay-tickets             │                                  │
     │    (kind, scopes, client key)         │                                  │
     │──────────────────────────────────────>│                                  │
     │ 2. ticket + machine Iroh node id      │                                  │
     │<──────────────────────────────────────│                                  │
     │                                                                          │
     │ 3a. QUIC to the machine's Iroh endpoint (preferred, short deadline)      │
     │══════════════════════════════════════════════════════════════════════════>
     │                                                                          │
     │ 3b. on timeout / no route: existing relay ticket and E2E channel          │
     │──────────────────────── wss://relay.stackspirit.io ─────────────────────>│
     │                                                                          │
     │ 4. the same framed request/response or blob transfer on either path      │
```

The API remains the discovery and authorisation point. It returns a short-lived ticket that names:

- the account and machine,
- the Iroh node id and any direct addresses (optional hints),
- the application scopes,
- the expiry and a `jti`.

The ticket is signed by the API and re-verified by the machine, exactly like the current relay
ticket. The Iroh node id is bound to the machine's Ed25519 identity in the API database, so a
ticket cannot point a client at an unrelated node.

## 4. Identity and trust

The machine already has an Ed25519 identity used for relay enrollment and ticket verification. The
first implementation reuses it as the Iroh node identity:

- The Iroh `SecretKey` is the existing machine key, held in the same private file with the same
  `0600` rules. There is no second long-lived key to rotate.
- The API stores `iroh_node_id` next to the existing `key_id` and `public_key`; a rotation updates
  both in one transaction.
- A desktop/CLI client generates an ephemeral Iroh key per process. Its public key is included in
  the ticket request and bound by the ticket, so the machine can reject a swapped client.
- The application still performs its own authentication: the ticket is not a substitute for the
  account token. A direct connection that presents a valid ticket but the wrong client key is
  rejected before any application frame is read.

Payload protection is layered:

1. QUIC/TLS protects the direct path in transit.
2. The existing end-to-end envelope (P-256 ECDH + HKDF-SHA256 + AES-256-GCM, see
   `relay-and-packaging.md` §6.5) protects the relay path, where the relay is a hop.
3. The application keeps the same envelope on the direct path. This costs little, keeps one
   framing implementation and means a future Iroh relay cannot read payloads either.

The API, the Iroh discovery service and the relay learn connection metadata (node ids, addresses,
timing, byte counts). They never learn a prompt, a tool output, a file body, a secret value or a
session log.

## 5. Transport selection

Each `RemoteTransport` gets three modes, configured globally and overridable per command:

| Mode | Behaviour |
|---|---|
| `auto` (recommended when enabled) | Race a direct attempt against a short timer (default 750 ms). If direct connects first, use it; otherwise open the relay channel and continue. A direct attempt that fails later must not cancel an in-flight relay request. |
| `relay` | Never open a UDP socket. Used by locked-down networks, CI and debugging. |
| `direct` | Require a direct path; report a typed `p2p_unavailable` error with a hint to use `--transport relay`. |

Selection rules:

- `--transport auto|relay|direct` on the CLI; `transport = "auto"` in the config; environment
  `STACKSPIRIT_TRANSPORT` / `VOWP_TRANSPORT` for the compatibility name.
- The desktop setting is stored per account/home, not per project.
- The machine can disable UDP ingress (`[p2p] enabled = false`), in which case the API omits the
  Iroh hints and clients go straight to the relay.
- `off` is the default in the first release. Enabling the feature is an explicit owner action.

Fallback is per request, not per process. A successful direct connection is cached for the life of
the process, but a later failure re-runs the selection instead of pinning a broken path.

## 6. Discovery and tickets

Extend the existing ticket endpoint rather than add a second authorisation system:

```http
POST /v1/relay-tickets
Authorization: Bearer <account access token>
Content-Type: application/json

{
  "machine_id": "mch_01JQ8Z6K4W5N7R9T2V3X4Y5Z6A",
  "kind": "desktop",
  "scopes": ["http.read", "session.read", "artifact.write"],
  "client_key": {"kty": "EC", "crv": "P-256", "x": "...", "y": "..."},
  "client_iroh_key": "<base64url Ed25519 public key>",
  "want_direct": true,
  "ttl_secs": 300
}
```

The response keeps every current field and adds:

```json
{
  "p2p": {
    "node_id": "<machine Iroh node id>",
    "addrs": ["<udp host:port>", "..."],
    "relay_url": "https://relay.stackspirit.io",
    "expires_at": "2026-11-26T22:05:00.000Z"
  }
}
```

Rules:

- `p2p` is absent when the machine has the feature off, has published no endpoint or the account
  plan disables it. Clients must treat absence as "use the relay" rather than an error.
- Address hints are hints. The client also asks Iroh's discovery service for the node id; an
  unreachable hint is not a failure.
- The ticket remains single-use within its lifetime (same `jti` cache). The direct transport
  presents the ticket in its first application frame, not in a URL.
- The API never returns a private key, a secret value, a session log or a file body.

## 7. Wire protocol

Application framing is unchanged between the relay and direct paths:

- ALPN: `stackspirit/transfer/1`.
- One bidirectional QUIC stream per request/response, with length-prefixed frames matching the
  existing `RemoteFrame` envelopes.
- Blob transfers use `iroh-blobs` with a BLAKE3 hash in the request and a resumable provider. The
  receiver verifies the hash before making the file visible.
- A direct connection starts with a `hello` frame carrying the ticket, the client ephemeral key and
  the protocol version. The machine verifies it before reading anything else.
- Unknown frame types and unsupported versions close the stream with the same typed error the relay
  path returns; they never become a panic or a silent no-op.

The relay client and the direct client implement the same `RemoteTransport` trait:

```rust
trait RemoteTransport {
    fn request(&mut self, frame: RemoteFrame) -> Result<RemoteFrame, TransportError>;
    fn open_blob(&mut self, hash: BlobHash, size: u64) -> Result<BlobWriter, TransportError>;
}
```

Command handlers, the desktop bridge and the MCP proxy depend on the trait, not on Iroh. This keeps
the dependency optional and makes the relay the reference implementation.

## 8. Desktop and CLI surfaces

Planned, behind an explicit gate:

- CLI global flag `--transport auto|relay|direct` on `ssh`, `session export|import`, `agent` and
  future artifact commands. `stackspirit doctor` gains a `p2p` check that reports "off", "ready",
  "direct" or "relay only" without printing an address or key.
- Desktop settings: a single **Direct connections** toggle with the honest states
  `Off`, `Automatic`, `Direct only`. The UI shows the observed path for the current session
  (`direct` or `relay`) and a short reason when it fell back. It never shows a raw IP address.
- No new button on the main surfaces. The transport is infrastructure, not a workflow.
- JSON output adds `"transport": "direct" | "relay"` to the affected documents. Existing fields
  keep their meaning; consumers that ignore the new field keep working.

## 9. Packaging and licensing

- Cargo feature `p2p` (off by default) on the CLI, desktop core and daemon.
- Exact-pinned `iroh` (and `iroh-blobs` only when artifact transfer lands). A workspace-level
  `[workspace.dependencies]` entry keeps the version in one place.
- `THIRD_PARTY_NOTICES.md` and `runtime/THIRD-PARTY-LICENSES.md` gain the Apache-2.0/MIT entry when
  the feature is first compiled into a release. The release build must record whether `p2p` was
  enabled.
- The default build's binary size and startup time must not change. A release job compares the
  feature-off binary before and after the dependency lands.
- The desktop's CSP does not need a new HTTPS origin for QUIC; if a discovery HTTP endpoint is
  added it must be listed explicitly, never `*`.

## 10. Security and failure modes

| Risk | Mitigation |
|---|---|
| A stolen machine key impersonates the machine | Same exposure as today; key file is `0600`, rotation is a single API transaction, and the ticket is still account-scoped. |
| A malicious API points a client at an attacker node | The ticket is signed and names the machine node id; the client verifies the machine signature before deriving keys. |
| A direct peer sends a huge frame | Length caps before allocation; the same caps as the relay path. |
| UDP is blocked | `auto` falls back within the deadline; `relay` never tries UDP. |
| Iroh public relay is unreachable | Direct and our relay are independent; `relay` mode bypasses Iroh's discovery relay entirely after the ticket. |
| Iroh pre-1.0 breakage | Exact pin, `P2pTransport` trait, feature off by default. |
| Direct path leaks an address to the UI or logs | No raw address in output; only `direct`/`relay`. |
| Half-open direct connection stalls a command | Per-request deadline; a failed direct attempt does not cancel the relay attempt. |
| Two paths deliver the same request twice | Request ids are idempotent at the application layer; a retry after fallback reuses the id. |

The fallback is deliberately **not** a silent downgrade of security: both paths carry the same
end-to-end envelope and the same account/machine authorisation.

## 11. Rollout

1. **Design and dependency spike (this document).** No shipping code, no default-on feature.
2. **API fields and machine identity binding.** Add `iroh_node_id` and the optional `p2p` ticket
   block behind a server flag; keep returning no `p2p` by default.
3. **CLI ↔ Home session export/import over direct QUIC.** One command family, `auto` and `relay`
   modes, the relay path as the reference. Gate: a loopback two-endpoint test and a forced-fallback
   test.
4. **Desktop ↔ Home.** Reuse the same transport trait; add the settings toggle and the honest path
   indicator.
5. **Machine ↔ machine and blob transfer.** Only after the first two slices have run in production
   with the feature explicitly enabled.
6. **Default decision.** The owner decides whether `auto` becomes the default. The code keeps
   `off` until that decision is recorded in `docs/HANDOFF-DESKTOP-UI.md`.

The existing `LocalOrchestratorGate` and `AsifGate` stay in place. This transport does not make the
local orchestrator or ASIF wiring live, and the UI must not claim otherwise.

## 12. Tests

- **Loopback pair.** Two Iroh endpoints in one test process exchange a framed request and a blob;
  no external network, no public discovery.
- **Forced fallback.** Bind a blackhole UDP address, set a 50 ms direct deadline and assert the
  relay mock receives the same frame exactly once.
- **Wrong key.** A client with a valid ticket but the wrong ephemeral key is rejected before the
  first application frame.
- **Replay.** The same ticket `jti` is refused on the second direct connection.
- **Frame caps.** Oversized and truncated frames produce the typed transport error, not a panic.
- **Feature off.** A default build has no `iroh` symbols and `doctor --json` reports `p2p: off`.
- **Relay parity.** The same command against the relay and direct transports returns the same JSON
  document (apart from the `transport` field).

## 13. Open owner decisions

- Whether StackSpirit runs its own Iroh discovery/DERP relay or relies on Iroh's public one for
  hole punching. The application fallback remains `wss://relay.stackspirit.io` either way.
- Whether the account plan gates direct connections, and what the free tier includes.
- Whether the machine key is reused directly or an Iroh subkey is derived and signed by it. The
  design prefers reuse for the first slice and derivation if key rotation becomes routine.
- Whether the desktop toggle belongs in account settings or per-home settings.

Until those decisions are recorded, the feature stays off and this document remains a design.
