# Relay and packaging — the public monorepo, install launchers and the StackSpirit relay

Status: **design proposal; no slice implemented by this page.** This is the deliverable for
`v6-8-pypi-npm-launchers-and-relay-design`. It proposes the target public monorepo, the npm and
PyPI launch paths, the GitHub Actions OIDC release workflow and the Stage 2 outbound WebSocket relay
with its cloud contract, threat model and seven independently mergeable slices. It does not promise
a release, tag, version bump or date, and it does not claim that any command or endpoint below
exists today.

The v6-6 packaging slice and the v6-7 docs cutover landed before this page. Their current shape is
the starting point, not the target:

| Area | In the tree today | This design |
|---|---|---|
| Repositories | `Stack-Spirit/stackspirit-engine` (private engine) and `Stack-Spirit/stackspirit` (private UI) | One public MIT `Stack-Spirit/stackspirit`; engine at the root, UI under `apps/` |
| npm | `packaging/npm-stackspirit` is `"private": true` and downloads a private archive in `postinstall` | Public `stackspirit` package plus four platform `optionalDependencies`; no install script |
| PyPI | `packaging/pypi-stackspirit` is a private console-script launcher with a `Private :: Do Not Upload` classifier | Public `stackspirit` platform wheels; `uvx stackspirit` installs the matching wheel |
| Release | `.github/workflows/release.yml` builds archives and a draft GitHub release; it publishes no package registry | The same draft gate, then OIDC trusted publishing to npm and PyPI; no long-lived registry token |
| Remote access | `crates/vowp-agent/src/wss.rs` dials the owner's own TLS reverse proxy; there is no hosted relay | A machine dials `wss://relay.stackspirit.io` outbound; the relay routes an end-to-end encrypted channel |
| Machine identity | `node.key` is the v0.4 agent identity; no cloud identity exists | A separate `relay.key` Ed25519 identity; the private key never leaves the machine |
| Cloud | none | `api.`, `relay.`, `app.` and `mcp.stackspirit.io`; Cloudflare Workers, Durable Objects and D1 |

The public VOWP release at `syver777/vowp` is deliberately outside this design. It keeps its name,
its 0.4.0-era public packaging and its own release line; no StackSpirit branding or private
repository content is pushed to it by these slices.

## 1. Scope, invariants and vocabulary

### 1.1 Scope

**Stage 1 — finish the launch.** One public MIT monorepo, thin npm and PyPI launchers with
per-platform artifacts, and a release workflow that builds the artifacts and publishes them with
GitHub OIDC. The user-visible promise is narrow and testable:

```sh
npx -y stackspirit --version
npx -y stackspirit mcp
uvx stackspirit --version
```

`npx -y stackspirit mcp` with no flags must start a usable read-only stdio MCP server, because the
current `vowp mcp` requires `--session`, `--surface` and `--token-file`. The npm promise is
therefore not packaging alone: §3.7 defines the additive no-flag mode and the one CLI change the
launcher slice owns.

**Stage 2 — foundation for the cloud.** Any always-on machine (a Contabo VPS, a home Mini PC behind
NAT, a Mac mini) opens one authenticated outbound WSS connection to `relay.stackspirit.io`. The
machine holds a per-machine Ed25519 identity and a private key that never leaves the machine. The
relay routes an end-to-end encrypted channel between the machine and an authorised client. The
clients are the hosted PWA at `app.stackspirit.io`, the hosted remote MCP server at
`mcp.stackspirit.io`, StackSpirit Desktop, and `stackspirit ssh`. The cloud stores public keys and
short-lived authorisation metadata, never SSH private keys and never plaintext channel payloads.

Stage 2 includes the Contabo cloud-init enrollment flow, the outbound SMTP block and an **optional**
encrypted R2 backup hook that is off by default.

### 1.2 Invariants

These bind every slice.

1. **The machine keeps its private keys.** `relay.key` and the user's SSH private keys never leave
   the machine. The relay and API store public keys only. There is no cloud-stored SSH private key
   and no cloud-held key that can impersonate a machine.
2. **The relay routes; it does not read.** Application payloads travel inside a P-256 ECDH /
   AES-256-GCM channel between the machine and the client. The relay sees control metadata and
   ciphertext, never prompts, tool output, file contents or SSH bytes.
3. **The home remains the only writer.** The relay connector forwards to the machine's existing
   loopback daemon; it never opens a database over the network and never becomes a second writer.
4. **Local behaviour is unchanged.** `stackspirit`, `vowp`, the `/api/v1` routes, the MCP tool names,
   the agent protocol and the on-disk data names stay compatible. A relay failure never breaks a
   local command.
5. **No public listener on the machine.** The connector only dials out. The relay and API are
   Cloudflare-hosted; the machine opens no inbound port.
6. **Default deny.** Remote HTTP writes, remote MCP writes, SSH and command execution are off until
   the owner enables the scope on the machine. Read-only fleet and session access is the v1 default.
7. **Publishing is human-gated.** A tag builds a draft GitHub release. npm and PyPI publish only
   after the owner publishes that release, from a protected `release` environment, with an OIDC
   token and no registry password.
8. **The public VOWP tree is separate.** No slice changes `syver777/vowp` or its archives.
9. **One version.** The engine workspace version is the product version; npm, PyPI, Homebrew and
   the UI package versions must match it before a release.

### 1.3 Vocabulary

| Term | Meaning |
|---|---|
| **machine** | A Linux or macOS host running `stackspirit up` and `stackspirit relay run`; it owns a `relay.key` and a machine record |
| **relay** | `wss://relay.stackspirit.io`; a Cloudflare Worker plus one Durable Object per machine that forwards control frames and opaque channel data |
| **API** | `https://api.stackspirit.io`; account auth, machine enrollment, machine credentials, relay tickets and audit |
| **client** | A web/PWA, hosted MCP server, desktop app or CLI that obtains a ticket and opens a channel to one machine |
| **ticket** | A short-lived ES256 JWT issued by the API, bound to one machine, one client ephemeral P-256 key and a scope set |
| **machine credential** | A longer-lived ES256 JWT issued by the API, bound to one machine public key; it authorises the machine to the relay |
| **channel** | One end-to-end encrypted multiplexed connection between a client and a machine, carrying HTTP, MCP or TCP streams |
| **stream** | One HTTP request, MCP call or TCP/SSH byte stream inside a channel |
| **scope** | `http.read`, `http.write`, `mcp.read`, `mcp.write`, `ssh` or `exec` |
| **relay connector** | `stackspirit relay run`; the machine-side process that dials the relay and serves channels |
| **remote surface** | The PWA, hosted MCP, desktop or CLI view of a machine; not a local VOWP surface and not a session lease holder |

## 2. Stage 1 — the public monorepo

### 2.1 Target layout

The engine becomes the repository root. That keeps the existing Cargo workspace, the root
`vowp-cli` package, the `include_str!` `framework/` files, `rust-toolchain.toml`, `scripts/release`
and every current path exactly where they are. The UI repository is imported under `apps/`, so the
UI keeps its own toolchain and the engine build does not acquire a second package manager.

```text
Stack-Spirit/stackspirit/
├── Cargo.toml                     # engine workspace, unchanged location
├── Cargo.lock
├── rust-toolchain.toml
├── rustfmt.toml
├── crates/                        # engine crates, unchanged
├── framework/                     # engine templates and prompts, unchanged
├── scripts/                       # install, release, tests, audit
├── docs/                          # engine design, guides and reference
├── packaging/
│   ├── npm/                       # new public launcher and platform packages
│   ├── pypi/                      # new public wheels
│   ├── homebrew/
│   └── legacy/                    # frozen public `vowp` npm/Homebrew/PyPI templates
├── apps/
│   ├── desktop/                   # imported Stack-Spirit/stackspirit UI tree
│   ├── web/                       # hosted PWA source (app.stackspirit.io)
│   └── README.md
├── cloudflare/
│   ├── api/                       # Cloudflare Worker + D1 migrations
│   ├── relay/                     # Cloudflare Worker + Durable Object
│   └── mcp/                       # hosted remote MCP Worker
├── deploy/
│   ├── cloud-init/                # Contabo user-data templates
│   ├── backup/                    # optional age + rclone R2 backup
│   └── systemd/                   # stackspirit.service, stackspirit-relay.service
├── .github/
│   ├── CODEOWNERS
│   └── workflows/{engine,packaging,cloudflare,apps,security,release}.yml
├── pnpm-workspace.yaml            # apps/web, cloudflare/*, packaging/npm/stackspirit, packaging/npm/platforms/*
├── README.md                      # product front door
├── LICENSE                        # MIT for first-party code
├── NOTICE
├── THIRD-PARTY-LICENSES.md
├── CONTRIBUTING.md
├── SECURITY.md
├── CODE_OF_CONDUCT.md
├── SUPPORT.md
└── AGENTS.md
```

`apps/desktop` and `apps/web` are imported history, not rewritten code. If the UI repository already
has an `apps/` directory, the import is tag-only and the paths stay as they are. If it has
`desktop/` and `web/` at its root, the filter places them at `apps/desktop` and `apps/web`. The
import script normalises only that top-level directory, not the UI source.

`packaging/npm` and `packaging/pypi` are new trees. The existing public `vowp` npm package and its
Homebrew formula move to `packaging/legacy/` and are frozen; they are not published again from this
monorepo. The new `stackspirit` package also installs the `vowp` and `vowp-daemon` commands, so a
user who installs both global packages would get two `vowp` shims; the README tells users to
uninstall the old package first.

### 2.2 Importing the two histories

The import is a one-time owner operation, not a normal slice diff. It runs on a clean workstation
with both private repositories cloned; it writes to a new private staging repository first.

```sh
# 1. Clone both repositories without hardlinks and record the source SHAs.
git clone --no-local git@github.com:Stack-Spirit/stackspirit-engine.git engine-import
git clone --no-local git@github.com:Stack-Spirit/stackspirit.git ui-import
git -C engine-import rev-parse HEAD > engine-import.sha
git -C ui-import rev-parse HEAD > ui-import.sha

# 2. Move the UI history below apps/ (unless it already has apps/) and
#    prefix its tags, so its `v1.2.3` tag cannot collide with a product tag.
if [ -d ui-import/apps ]; then
  git -C ui-import filter-repo --tag-rename '':'ui-'
else
  git -C ui-import filter-repo --to-subdirectory-filter apps --tag-rename '':'ui-'
fi

# 3. Merge the unrelated histories in the engine clone.
git -C engine-import remote add ui ../ui-import
git -C engine-import fetch ui
git -C engine-import merge --allow-unrelated-histories --no-ff ui/main \
  -m "monorepo: import UI history under apps/"

# 4. Verify before pushing: both roots are reachable, tags are prefixed and
#    `git log --follow` still crosses the import commit.
test -f engine-import/Cargo.toml
test -f engine-import/apps/desktop/README.md        # path in the imported UI tree
git -C engine-import log --follow --oneline -- apps/desktop/README.md | tail -1

# 5. Push to a new private staging repository, never force-push the old ones.
git -C engine-import remote add staging git@github.com:Stack-Spirit/stackspirit-staging.git
git -C engine-import push staging main --tags
```

`git filter-repo` rewrites the UI commit ids; the staging repository must carry the generated commit
map as a release artifact, not as a committed file. The engine history is not rewritten, so its
`v*` tags and commit ids remain valid. After the staging repository passes CI and the license audit,
the owner renames it to `Stack-Spirit/stackspirit`, renames the old UI repository to
`Stack-Spirit/stackspirit-ui-archive` and archives the old engine repository. GitHub redirects the
old URLs; the archives stay read-only for at least 90 days.

The import script lives at `scripts/monorepo/import-history.sh` and accepts `ENGINE_REPO`,
`UI_REPO`, `DEST` and `STAGING_REMOTE` so its behaviour can be tested on two tiny temporary
repositories. It never pushes to `main`, never force-pushes, and refuses a dirty destination.

### 2.3 Licence, provenance and public-history audit

The destination is public MIT, so the audit is a gate before the repository becomes public:

- `LICENSE` is the MIT text; every first-party package manifest declares `MIT`. Third-party code
  keeps its own licence in `THIRD-PARTY-LICENSES.md`.
- `scripts/monorepo/check-licenses.sh` checks every workspace crate (`license.workspace = true`),
  every npm package (`"license": "MIT"`), every Python package (`License-Expression: MIT`) and the
  Rust third-party bundle.
- The UI tree is checked before import: a file that cannot be MIT is either relicensed by its owner
  or kept out of the public tree. A UI file without a compatible licence blocks the cutover.
- The existing public-audit tools are the starting point: `scripts/audit/scan.sh`,
  `scripts/audit/public-tree-check.sh`, `scripts/audit/licenses.sh` and the owner runbook
  `scripts/audit/go-public.md`. The monorepo slice updates `scripts/audit/public-tree.txt` for the
  new top-level paths and runs the checks on the staging clone. It does **not** use
  `squash-for-public.sh`, because the goal preserves both histories; a full-history scan
  (`gitleaks detect --source . --log-opts=--all`, or an equivalent historical pass of `scan.sh`)
  is the added step. A finding stops the cutover; the owner rotates the secret, rewrites the
  affected history with `git filter-repo --replace-text`, and repeats the audit. No secret value is
  printed in the report.
- `.vowp/` remains untracked. Job briefs, reports, checkpoints and worktrees are not public history.
- The root `README.md` is the product front door; the engine's existing README content moves under
  `docs/` links or is merged, not silently deleted. The UI README is linked from `apps/README.md`.
- The workspace `repository` and `homepage` metadata point at
  `https://github.com/Stack-Spirit/stackspirit` in the public tree. The crate names, library names
  and public VOWP release remain unchanged; this is package metadata, not a rename.

### 2.4 CI and path ownership

The monorepo does not run every toolchain on every change. Six workflows use `paths` filters and a
small required-check aggregator:

| Workflow | Runs when | Checks |
|---|---|---|
| `engine.yml` | `crates/**`, `Cargo.*`, `framework/**`, `scripts/**`, `docs/**` | `cargo fmt`, clippy, `cargo test --workspace`, docs links/commands, reference check |
| `packaging.yml` | `packaging/**` | npm tests and `npm pack`; PyPI tests, wheel build and offline install |
| `cloudflare.yml` | `cloudflare/**` | TypeScript typecheck, vitest with Miniflare, cross-language relay vectors |
| `apps.yml` | `apps/**` | UI unit tests and build; no network except the package registry for dependencies |
| `security.yml` | always | gitleaks, `cargo audit`, npm audit, licence check, secret scan |
| `release.yml` | tags and release events | the release pipeline in §5 |

`CODEOWNERS` gives the engine, packaging, cloudflare, apps and docs trees separate reviewers. A
required `ci` check aggregates the path-filtered jobs so branch protection does not depend on a
skipped workflow. The merge queue rebases and re-runs the aggregate before `main` moves.

`scripts/monorepo/check-versions.sh` reads the workspace version from `Cargo.toml` and compares it
with the npm root and platform packages, the PyPI metadata and the Homebrew formula. A mismatch
fails CI. The release workflow repeats the check against the tag.

### 2.5 What stays where

- The engine workspace stays at the root. Moving it into `engine/` would touch the root
  `vowp-cli` package, its `include_str!` paths, the release archive layout and every documented
  command for no user-visible benefit.
- The UI keeps its own package manager and build. The engine never imports UI source; the only
  shared contract is the daemon `/api/v1` API and the relay client. If the desktop UI contains a
  Rust/Tauri crate, that crate declares its own `[workspace]` (or is excluded from the root
  workspace) so `cargo test --workspace` at the root never treats the UI as an engine member.
- The `.vowp/` project state directory, `vowp.db`, `project.db`, tmux names and wire format strings
  stay unchanged. The monorepo is a code layout change, not a data migration.
- The public VOWP repository remains a separate release; it is not a subtree of this monorepo.

### 2.6 Rollback and failure behaviour

- **Import is wrong.** The staging repository is discarded; the two original repositories were
  never modified, so nothing is lost.
- **Public audit fails.** The repository stays private; the finding is fixed on staging and the
  audit repeats. A secret that cannot be removed from history blocks publication until the owner
  rotates it and rewrites history.
- **UI and engine toolchains conflict.** The UI stays under `apps/` with its own lockfile; a root
  lockfile is not created for it. If a shared dependency creates a conflict, the UI build runs in
  its directory and the engine CI does not install UI dependencies.
- **Old repository URLs.** GitHub redirects them after the rename; the archives stay read-only for
  90 days. A release note names the new URL.

## 3. Stage 1 — the npm launcher

### 3.1 Package graph

```text
stackspirit (root, public, MIT)
├── bin/stackspirit.js          -> bin/stackspirit
├── bin/vowp.js                 -> bin/vowp
├── bin/stackspirit-daemon.js   -> bin/stackspirit-daemon
├── bin/vowp-daemon.js          -> bin/vowp-daemon
└── optionalDependencies
    ├── @stackspirit/linux-x64       (bin/stackspirit, vowp, stackspirit-daemon, vowp-daemon)
    ├── @stackspirit/linux-arm64
    ├── @stackspirit/darwin-x64
    └── @stackspirit/darwin-arm64
```

The root package contains no binary and no `postinstall`. npm installs exactly one platform package
because each one declares `os`, `cpu` and (for Linux) `libc`. `npx -y stackspirit` downloads the root
package and the matching platform package from the registry, then runs the root launcher. `npx -y
stackspirit mcp` runs the same launcher with the argument `mcp`; the launcher has no special case.
The CLI's no-flag read-only mode (§3.7) is what makes that invocation useful without a session.

### 3.2 Root `package.json` (exact fields)

```json
{
  "name": "stackspirit",
  "version": "0.6.0",
  "description": "StackSpirit: local-first supervisor for AI coding agents",
  "license": "MIT",
  "homepage": "https://stackspirit.io",
  "repository": {
    "type": "git",
    "url": "git+https://github.com/Stack-Spirit/stackspirit.git",
    "directory": "packaging/npm/stackspirit"
  },
  "bugs": { "url": "https://github.com/Stack-Spirit/stackspirit/issues" },
  "engines": { "node": ">=18" },
  "bin": {
    "stackspirit": "bin/stackspirit.js",
    "vowp": "bin/vowp.js",
    "stackspirit-daemon": "bin/stackspirit-daemon.js",
    "vowp-daemon": "bin/vowp-daemon.js"
  },
  "files": ["bin/", "lib/", "README.md", "LICENSE"],
  "optionalDependencies": {
    "@stackspirit/linux-x64": "0.6.0",
    "@stackspirit/linux-arm64": "0.6.0",
    "@stackspirit/darwin-x64": "0.6.0",
    "@stackspirit/darwin-arm64": "0.6.0"
  },
  "publishConfig": { "access": "public" },
  "scripts": { "test": "node --test test/*.test.js" }
}
```

There is deliberately no `scripts.install`, `scripts.preinstall` or `scripts.postinstall`. The
package is installable with `npm ci --ignore-scripts`, and no install step reaches the network
beyond the registry.

### 3.3 Platform `package.json` (exact fields)

Each platform package is a tiny package with no dependencies and no scripts:

```json
{
  "name": "@stackspirit/linux-x64",
  "version": "0.6.0",
  "description": "StackSpirit prebuilt binaries for linux/x64 (glibc)",
  "license": "MIT",
  "homepage": "https://stackspirit.io",
  "repository": {
    "type": "git",
    "url": "git+https://github.com/Stack-Spirit/stackspirit.git",
    "directory": "packaging/npm/platforms/linux-x64"
  },
  "os": ["linux"],
  "cpu": ["x64"],
  "libc": ["glibc"],
  "files": ["bin/", "LICENSE"],
  "publishConfig": { "access": "public" }
}
```

The other three packages replace `os`, `cpu` and `libc`: `linux-arm64` is `linux`/`arm64`/`glibc`;
`darwin-x64` and `darwin-arm64` are `darwin` with no `libc`. Linux musl is not published in the
first release; an Alpine user gets the explicit unsupported-platform message from the launcher,
which points at a source build. Adding `@stackspirit/linux-x64-musl` later is additive.

### 3.4 Launcher algorithm

`packaging/npm/stackspirit/lib/launch.js` is the only executable logic:

```js
const PACKAGES = Object.freeze({
  "linux-x64": "@stackspirit/linux-x64",
  "linux-arm64": "@stackspirit/linux-arm64",
  "darwin-x64": "@stackspirit/darwin-x64",
  "darwin-arm64": "@stackspirit/darwin-arm64",
});

function resolveBinary(name) {
  const key = `${process.platform}-${process.arch}`;
  const pkg = PACKAGES[key];
  if (!pkg) throw new Error(unsupportedMessage(process.platform, process.arch));
  if (process.platform === "linux" && isMusl()) {
    throw new Error(unsupportedMessage(process.platform, process.arch, { musl: true }));
  }
  let manifest;
  try {
    manifest = require.resolve(`${pkg}/package.json`);
  } catch {
    throw new Error(
      `the optional platform package ${pkg} is not installed; ` +
      `run \`npm install\` again without --omit=optional`,
    );
  }
  const root = require("../package.json");
  const platform = require(manifest);
  if (platform.version !== root.version) {
    throw new Error(
      `version mismatch: ${root.name} ${root.version} needs ${pkg} ${root.version}, ` +
      `found ${platform.version}; delete node_modules and reinstall`,
    );
  }
  const binary = path.join(path.dirname(manifest), "bin", name);
  if (!fs.existsSync(binary)) {
    throw new Error(`${pkg} does not contain bin/${name}`);
  }
  return binary;
}
```

`launch(name)` spawns the binary with `process.argv.slice(2)`, inherits stdio, forwards `SIGINT`,
`SIGTERM` and `SIGHUP`, and exits with the child's code or `128 + signal`. The four `bin/*.js`
entry points are one line each and call `launch("stackspirit")`, `launch("vowp")`,
`launch("stackspirit-daemon")` or `launch("vowp-daemon")`. The launcher spawns the `vowp` file for
the legacy command; the binary decides its product identity from its executable name, and the
platform package contains both names as separate files built from the same Rust package.

### 3.5 Building the packages

`scripts/release/build-npm-packages.sh` takes a version and either the four release archive files or
four directories of built binaries:

```sh
scripts/release/build-npm-packages.sh \
  --version "$VERSION" \
  --target linux-x64=dist/stackspirit-$VERSION-x86_64-unknown-linux-gnu.tar.gz \
  --target linux-arm64=... \
  --target darwin-x64=... \
  --target darwin-arm64=... \
  --out dist/npm
```

For each target it:

1. Extracts exactly `stackspirit`, `vowp`, `stackspirit-daemon` and `vowp-daemon` from the archive.
2. Copies the platform `package.json` template, the root `LICENSE` and the four binaries with mode
   `0755`.
3. Runs `npm pack --pack-destination dist/npm` in the platform directory.
4. Copies the root package, runs `npm pack`, and writes `SHA256SUMS`.
5. Refuses a missing binary, a symlink, a mode other than `0755`, a tarball member outside `bin/`
   or `package.json`, a version mismatch or a tarball over 64 MiB.

The generated tarballs are the only inputs the publish job uses. No build step runs during
`npm publish`.

### 3.6 Tests and gates

The tests are offline and use temporary directories and fake binaries.

| Test | What it proves | What fails if absent |
|---|---|---|
| `launcher_resolves_the_platform_package` | a fake `node_modules/@stackspirit/<platform>/bin/stackspirit` is found from the root launcher | a hard-coded `vendor/` path or a wrong package mapping |
| `launcher_passes_arguments_and_exit_code` | argv and exit code 7 reach the fake binary | argument loss or exit-code rewriting |
| `launcher_reports_missing_optional_dependency` | `--omit=optional` produces the reinstall message and exit 1 | a stack trace or a silent success |
| `launcher_refuses_a_version_mismatch` | root 0.6.0 + platform 0.5.1 is refused | a mixed-version binary runs |
| `root_package_has_no_install_script` | no `preinstall`, `install` or `postinstall` | a hidden download step returns |
| `platform_packages_are_os_and_cpu_scoped` | exactly one package can match a platform | two packages install the same `vowp` shim |
| `packed_tarballs_contain_only_runtime_files` | `npm pack --json` lists only `bin/`, `lib/`, README and licence | tests, source or a private archive leak into the package |
| `npx_style_install_runs_the_fake_binary` | a temp project with the packed tarballs runs `npx --no-install stackspirit --version` | the out-of-box promise is unproven |
| `npx_style_install_runs_mcp_with_arguments` | the same install runs `stackspirit mcp` with the argument intact | `npx -y stackspirit mcp` is broken |
| `mcp_without_a_session_is_read_only` | a real `stackspirit mcp` child answers `initialize` and lists only the five read tools | the no-flag command exits with a missing-flag error |
| `mcp_without_a_session_refuses_a_write` | a write call returns `-32601` and no store write occurs | a read-only server silently opens a write path |

**Gates.**

```sh
node --test packaging/npm/test/*.test.js
bash scripts/tests/npm.sh
npm pack --dry-run --json packaging/npm/stackspirit/package.json
```

The public `vowp` npm package in `packaging/legacy/npm-vowp` stays frozen at its last published
version. Its README tells users to uninstall it and install `stackspirit`; it is not part of the
publish job.

### 3.7 `mcp` without a session

`npx -y stackspirit mcp` has no flags, so it cannot use today's session/surface/token mode. The
design adds one additive mode:

- If `--agent-socket` is absent and none of `--session`, `--surface` and `--token-file` is given,
  `stackspirit mcp` serves a **read-only** stdio MCP server against the local home. It discovers
  the home exactly like `stackspirit status`.
- It answers `initialize` with `serverInfo.name: "stackspirit-engine"` and `protocolVersion` from
  the request; `ping` and `tools/list` work.
- `tools/list` returns only read tools that need no session: `fleet_status`, `fleet_queue`,
  `inbox_list`, `capacity_report` and `session_tail`. `session_tail` still takes the session id as
  its argument.
- A `tools/call` for a write tool returns JSON-RPC error `-32601` with a message saying the full
  server needs `--session`, `--surface` and `--token-file`. No write path is opened and no lease is
  checked or bypassed.
- Supplying any one of the three flags switches to the existing full mode and keeps its current
  validation; a partial group is an error, not a silent read-only fallback.
- The read-only server never opens a writable store, never creates a surface or lease and never
  logs a prompt or tool result.

This is the one CLI change in the npm slice. It is additive: every existing `vowp mcp` invocation
with a session behaves exactly as before, and the 15 tool names do not change.

## 4. Stage 1 — the PyPI wheels

### 4.1 Distribution and wheel set

The distribution name is `stackspirit`. PyPI receives four platform wheels for each release and no
sdist:

```text
stackspirit-0.6.0-py3-none-manylinux_2_35_x86_64.whl
stackspirit-0.6.0-py3-none-manylinux_2_35_aarch64.whl
stackspirit-0.6.0-py3-none-macosx_10_12_x86_64.whl
stackspirit-0.6.0-py3-none-macosx_11_0_arm64.whl
```

`uvx stackspirit` lets uv select the compatible wheel. On an unsupported platform (musl, Windows,
32-bit) there is no matching distribution; the error names `stackspirit` and the supported tags.
There is no source distribution because installing from source would require a Rust toolchain and
would not deliver the out-of-box promise.

### 4.2 Wheel contents

Every wheel has the same Python files and four binaries:

```text
stackspirit/__init__.py
stackspirit/launcher.py
stackspirit/bin/stackspirit
stackspirit/bin/vowp
stackspirit/bin/stackspirit-daemon
stackspirit/bin/vowp-daemon
stackspirit-0.6.0.dist-info/METADATA
stackspirit-0.6.0.dist-info/WHEEL
stackspirit-0.6.0.dist-info/entry_points.txt
stackspirit-0.6.0.dist-info/RECORD
stackspirit-0.6.0.dist-info/licenses/LICENSE
```

`WHEEL`:

```text
Wheel-Version: 1.0
Generator: stackspirit-build-wheel 0.6.0
Root-Is-Purelib: false
Tag: py3-none-manylinux_2_35_x86_64
```

`entry_points.txt`:

```text
[console_scripts]
stackspirit = stackspirit.launcher:main_stackspirit
vowp = stackspirit.launcher:main_vowp
stackspirit-daemon = stackspirit.launcher:main_stackspirit_daemon
vowp-daemon = stackspirit.launcher:main_vowp_daemon
```

`METADATA` uses `Metadata-Version: 2.4`, `Name: stackspirit`, the PEP 440 version,
`License-Expression: MIT`, `Requires-Python: >=3.9` and the public project URLs. `RECORD` hashes
every member with SHA-256 and leaves the `RECORD` line itself empty. ZIP timestamps are fixed at
1980-01-01 for reproducible wheels; binaries are stored with mode `0755`.

### 4.3 Launcher

`stackspirit/launcher.py` is tiny and has no third-party imports:

```python
import os
import sys
from pathlib import Path

NAMES = ("stackspirit", "vowp", "stackspirit-daemon", "vowp-daemon")


def bin_dir() -> Path:
    override = os.environ.get("STACKSPIRIT_BIN_DIR")
    return Path(override) if override else Path(__file__).resolve().parent / "bin"


def run(name: str) -> None:
    if name not in NAMES:
        raise SystemExit(f"stackspirit: unknown binary: {name}")
    binary = bin_dir() / name
    if not binary.is_file():
        sys.stderr.write(f"stackspirit: the {name} binary is not installed at {binary}\n")
        raise SystemExit(1)
    os.execv(str(binary), [name, *sys.argv[1:]])


def main_stackspirit() -> None:
    run("stackspirit")
# ... one small main_* function per name
```

`STACKSPIRIT_BIN_DIR` exists only for the offline tests; a normal wheel ignores it. `argv[0]` is
the command name so `vowp` keeps its compatibility identity. `os.execv` replaces the Python process
so signals and the terminal belong to the binary.

### 4.4 Build backend

The wheel is built by a small PEP 517 backend at `packaging/pypi/stackspirit/backend.py`, not by
setuptools. The backend implements `get_requires_for_build_wheel` (returns `[]`),
`prepare_metadata_for_build_wheel` and `build_wheel`. `build_wheel` reads two config settings:

```sh
python3 -m build --wheel --no-isolation \
  --config-setting bin-dir=target/x86_64-unknown-linux-gnu/release \
  --config-setting platform-tag=manylinux_2_35_x86_64 \
  packaging/pypi/stackspirit
```

It stages the four binaries, writes the metadata files, computes `RECORD`, and names the archive
`stackspirit-<pep440>-py3-none-<platform-tag>.whl`. It refuses a missing binary, a binary that is
not executable, a tag that does not match the target, a version that is not final and a wheel over
64 MiB. `scripts/release/build-pypi-wheels.sh` maps each release target to its tag and calls the
backend four times.

`pyproject.toml` declares the backend:

```toml
[build-system]
requires = []
build-backend = "backend"
backend-path = ["."]

[project]
name = "stackspirit"
version = "0.6.0"
requires-python = ">=3.9"
license = "MIT"
```

The version is checked against `Cargo.toml` by `scripts/monorepo/check-versions.sh`. The PEP 440
mapping is exact: `X.Y.Z` -> `X.Y.Z`, `X.Y.Z-alpha.N` -> `X.Y.ZaN`, `X.Y.Z-beta.N` -> `X.Y.ZbN`,
`X.Y.Z-rc.N` -> `X.Y.ZrcN`, `X.Y.Z-dev` -> `X.Y.Z.dev0`. Only `X.Y.Z` is publishable.

### 4.5 Platform tags and support

| Target | Wheel tag | Minimum |
|---|---|---|
| `x86_64-unknown-linux-gnu` | `manylinux_2_35_x86_64` | glibc 2.35 (Ubuntu 22.04) |
| `aarch64-unknown-linux-gnu` | `manylinux_2_35_aarch64` | glibc 2.35 |
| `x86_64-apple-darwin` | `macosx_10_12_x86_64` | macOS 10.12 |
| `aarch64-apple-darwin` | `macosx_11_0_arm64` | macOS 11 |

The Linux floor matches the current release runners. Moving to `manylinux_2_28` is a build-container
change, not a protocol change; it can happen in a later release. macOS binaries are ad-hoc signed in
CI; a Developer ID signature and notarisation are an owner action if Gatekeeper blocks them. The
README states the supported matrix and points musl and Windows users at a source build.

### 4.6 Tests and gates

| Test | What it proves | What fails if absent |
|---|---|---|
| `distribution_name_and_version_map` | `stackspirit` and the PEP 440 mapping | a private classifier or a wrong version |
| `wheel_tag_matches_the_target` | `WHEEL` and the filename agree | `uvx` installs the wrong architecture |
| `wheel_contains_all_four_binaries_and_no_source` | exact member list and modes | a missing daemon or a Rust source tree |
| `console_script_execs_the_binary` | a fake binary sees `argv[0]` and args | a Python wrapper that loses the command name |
| `pip_install_offline_runs_the_fake_binary` | `pip install --no-index --find-links` in a temp venv | the wheel is not self-contained |
| `uvx_style_run_uses_the_wheel` | `uv tool run --from <wheel> stackspirit --version` | the uvx promise is unproven |
| `unsupported_platform_has_no_wheel` | tags do not cover musl/Windows | a binary that fails at runtime instead |
| `build_is_reproducible` | two builds have identical member hashes | timestamps or ordering drift |

**Gates.**

```sh
python3 -m unittest discover -s packaging/pypi/tests
bash scripts/tests/pypi.sh
python3 -m build --wheel --no-isolation packaging/pypi/stackspirit
```

`uv` is installed in CI for the `uvx` test; the local gate skips that test with a clear message only
when `uv` is absent, and CI fails if it is absent.

## 5. Stage 1 — the OIDC release workflow

### 5.1 Publication gate

A tag never publishes directly. The safe sequence is:

1. Push a `v<version>` tag on a protected commit.
2. `release.yml` builds archives, npm tarballs and wheels, creates a **draft** GitHub release and
   uploads every artifact plus `SHA256SUMS`.
3. The owner reviews the draft and publishes it.
4. The `release: published` event runs the publish jobs, which use GitHub OIDC to obtain
   short-lived credentials for npm and PyPI. No `NPM_TOKEN`, `PYPI_API_TOKEN` or password exists.
5. After publishing, a verification job runs `npx -y stackspirit@<version> --version` and
   `uvx stackspirit@<version> --version` on clean runners.

A manual `workflow_dispatch` run defaults to a dry build and does not publish. It can publish only
with `publish: true`, and the publish jobs still require the protected `release`/`npm`/`pypi`
environments with owner approval.

### 5.2 Workflow shape

```yaml
name: Release
on:
  push:
    tags: ["v*"]
  release:
    types: [published]
  workflow_dispatch:
    inputs:
      version: { type: string }
      publish: { type: boolean, default: false }

permissions:
  contents: read

concurrency:
  group: release-${{ github.ref }}
  cancel-in-progress: false

jobs:
  prepare:        # version/tag/manifest check; rejects a non-final publish version
  build:          # 4-target matrix; archives, npm tarballs, wheels, smoke tests
  checksums:      # SHA256SUMS, attestations, artifact upload
  github-release: # only on a tag push; creates a draft; never publishes registries
  publish-npm:    # release event or publish=true; environment: npm; id-token: write
  publish-pypi:   # release event or publish=true; environment: pypi; id-token: write
  verify-install: # npx and uvx against the published versions
```

The `build` matrix is the existing native-runner matrix (`ubuntu-22.04`, `ubuntu-22.04-arm`,
`macos-15-intel`, `macos-15`). Each job builds `stackspirit`, `vowp`, `stackspirit-daemon` and
`vowp-daemon`, runs `--version` on both CLI names, packages the archive with
`scripts/release/package-archive.sh`, builds the matching npm platform package with
`scripts/release/build-npm-packages.sh`, and builds the matching wheel with
`scripts/release/build-pypi-wheels.sh`. The root npm tarball is assembled once in `checksums` from
the four platform tarballs.

The publish jobs download the exact artifacts from the draft release, re-verify `SHA256SUMS`, and
never rebuild. The npm job runs `npm@11` (trusted publishing requires a current npm), publishes the
four platform packages first and the root package last, with `--provenance --access public`.
The PyPI job runs `pypa/gh-action-pypi-publish` with `packages-dir: dist/pypi`; it uses the
`pypi` environment and `id-token: write`.

The two publish jobs have this exact permission and environment contract:

```yaml
publish-npm:
  needs: [prepare, checksums]
  if: github.event_name == 'release' || inputs.publish == true
  environment: npm
  permissions:
    contents: read
    id-token: write
  steps:
    - uses: actions/checkout@<sha>   # ref: ${{ needs.prepare.outputs.tag }}
    - uses: actions/download-artifact@<sha>
    - run: |
        set -euo pipefail
        npm install -g npm@11
        for pkg in dist/npm/platform/*.tgz; do
          npm publish "$pkg" --provenance --access public
        done
        npm publish dist/npm/stackspirit-*.tgz --provenance --access public

publish-pypi:
  needs: [prepare, checksums]
  if: github.event_name == 'release' || inputs.publish == true
  environment: pypi
  permissions:
    contents: read
    id-token: write
  steps:
    - uses: actions/checkout@<sha>   # ref: ${{ needs.prepare.outputs.tag }}
    - uses: actions/download-artifact@<sha>
    - uses: pypa/gh-action-pypi-publish@<sha>
      with:
        packages-dir: dist/pypi
```

`<sha>` stands for the pinned 40-character commit SHA in the real file. The environment names
`npm` and `pypi` must match the trusted-publisher configuration; a different name fails OIDC
before upload.

### 5.3 Permissions and pinning

- Top-level `permissions: contents: read`.
- `github-release` adds `contents: write` only.
- `publish-npm` and `publish-pypi` add `id-token: write` only.
- Every third-party action is pinned to a commit SHA with a comment naming the release tag, as the
  current workflow already does.
- No workflow step prints an environment variable that may hold a token.
- `npm publish` and the PyPI action are the only steps that can publish; the build jobs have no
  registry credentials.

### 5.4 Provenance, checksums and release notes

- `SHA256SUMS` covers every archive, npm tarball and wheel.
- `actions/attest-build-provenance` attests the archives and wheels; npm provenance is generated
  by `npm publish --provenance`; PyPI trusted publishing records its own attestation.
- The draft release body comes from `docs/release/<version>.md` when present, else `CHANGELOG.md`.
- The release includes the four archives, the four platform npm tarballs, the root npm tarball, the
  four wheels and `SHA256SUMS`.
- Homebrew formula updates are generated as an artifact, not pushed to a tap by the workflow. The
  owner applies them with `packaging/homebrew/update.sh`.

### 5.5 Version rules

The workspace version in `Cargo.toml` is the only source. The `prepare` job rejects:

- a tag that is not `v<version>` for the workspace version;
- a non-final version on a publish path (`-dev`, `-alpha`, `-beta`, `-rc`);
- a mismatch between Cargo, npm root/platform packages, PyPI metadata and Homebrew;
- a release event whose tag does not point at a commit on `main` or whose release is still a draft;
- a version already present on npm or PyPI (the job checks before publishing and fails rather than
  overwriting).

The desktop installer is not built by this workflow. It keeps its own UI release job and consumes
the same tag; a finished Stage 1 release can ship the CLI/daemon packages before the desktop
installer if the UI job is not ready, as long as the version file agrees.

### 5.6 Failure modes

| Failure | Behaviour |
|---|---|
| Build fails on one target | No release is created; the other matrix jobs stop or finish but nothing publishes |
| Draft release is wrong | Owner deletes it and re-tags; registries were never touched |
| npm publish succeeds, PyPI fails | Re-run only PyPI; npm versions are immutable. A retry never overwrites or silently skips an existing version; the owner publishes a patch if the npm side is wrong |
| npm scope or PyPI publisher missing | The publish job fails before upload with the exact trusted-publisher setup message; the draft remains |
| OIDC token unavailable | The job fails closed; no fallback to a stored token exists |
| `npx` smoke fails | The release is marked failed; the owner can deprecate a bad version and publish a patch |

### 5.7 Tests and gates

`scripts/release/test-workflow.sh` parses `release.yml` and asserts:

- `id-token: write` appears only in the two publish jobs;
- no `NPM_TOKEN`, `PYPI_API_TOKEN`, `TWINE_PASSWORD` or `CARGO_REGISTRY_TOKEN` string appears;
- both publish jobs declare `environment:`;
- the GitHub release job is draft-only and cannot publish registries;
- every `uses:` line has a 40-character commit SHA;
- the build matrix is exactly the four supported targets;
- the version check rejects every mutation of a test manifest.

**Gates.**

```sh
bash scripts/release/test-workflow.sh
bash scripts/release/check-packages.sh
bash scripts/tests/run.sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

The job itself never publishes. The report states that publication is owner-gated and was not run.

## 6. Stage 2 — the relay architecture

### 6.1 Topology

```text
                       Cloudflare edge
  ┌──────────────────────────────────────────────────────────────────┐
  │  app.stackspirit.io (Pages)   mcp.stackspirit.io (Worker)        │
  │            │                         │                           │
  │            └───────────┬─────────────┘                           │
  │                        ▼                                         │
  │  api.stackspirit.io (Worker + D1) ── JWKS, machine registry      │
  │                        │                                         │
  │                        ▼                                         │
  │  relay.stackspirit.io (Worker + Durable Object per machine)      │
  │        ▲ machine WSS                    ▲ client WSS             │
  └────────┼────────────────────────────────┼────────────────────────┘
           │ outbound 443 only              │ ticket-auth WSS
  ┌────────┴─────────┐              ┌───────┴───────────────────────┐
  │ machine          │              │ clients                       │
  │ stackspirit up   │              │ PWA / Desktop / CLI / Hosted  │
  │ stackspirit relay│              │ MCP                           │
  │  relay.key       │              └───────────────────────────────┘
  │  loopback daemon │
  │  sshd on loopback │
  └──────────────────┘

  The relay forwards control frames and opaque E2E ciphertext.
  The machine and client share P-256 ECDH keys; the relay cannot decrypt.
```

| Component | Runs on | Owns | Never holds |
|---|---|---|---|
| `stackspirit relay run` | machine | `relay.key`, machine credential, connection to relay, local daemon/SSH forward | client private keys, plaintext of other clients |
| `relay.stackspirit.io` | Cloudflare Worker + DO | WebSocket sessions, channel routing, presence, backpressure | machine/client private keys, plaintext payloads, prompts, tool output |
| `api.stackspirit.io` | Cloudflare Worker + D1 | accounts, machines, public keys, enrollment tokens (hashes), tickets, audit | private keys, channel payloads, SSH keys, secret values |
| `app.stackspirit.io` | Cloudflare Pages | browser UI, ephemeral P-256 key per connection | machine private key, long-lived relay ticket |
| `mcp.stackspirit.io` | Cloudflare Worker | OAuth resource server, MCP JSON-RPC facade | machine private key, tool results after the request |
| Desktop / CLI | owner device | account token in OS keyring, ephemeral P-256 key | machine private key, SSH private key in the cloud |

### 6.2 Machine identity and enrollment

The machine identity is a **new** Ed25519 key at `<home>/relay.key` (mode 0600), with
`<home>/relay.pub` (0644). It is deliberately separate from `node.key`: rotating the relay identity
must not disturb the v0.4 home-agent enrollment, and the relay key is used only with domain-separated
signatures. The key is generated locally by `stackspirit relay enroll`; it never leaves the machine.

```
relay.key      32-byte Ed25519 secret, lowercase hex, 0600
relay.pub      32-byte Ed25519 public key, lowercase hex, 0644
relay.json     0600 JSON: machine_id, account_id, key_id, credential, API/relay URLs
```

`relay.json`:

```json
{
  "v": 1,
  "api_url": "https://api.stackspirit.io",
  "relay_url": "wss://relay.stackspirit.io/v1/machine",
  "machine_id": "mch_01JQ8Z6K4W5N7R9T2V3X4Y5Z6A",
  "account_id": "acct_01JQ9B7M5X6P8S1T3W4Y5Z6A7B",
  "key_id": "ed25519:<64 lowercase hex>",
  "credential": "<ES256 JWT>",
  "credential_expires_at": "2026-11-26T12:00:00.000Z",
  "name": "contabo-1",
  "project": "vowp",
  "ssh_user": "stackspirit",
  "registered_at": "2026-11-25T12:00:00.000Z"
}
```

`key_id` is the lowercase hex public key itself; the `ed25519:` prefix is a display and domain
separator. `machine_id`, `account_id` and ticket `jti` values are prefixed ULIDs
(`mch_`, `acct_`, `jti_`) with the existing 26-character Crockford alphabet.

The matching node `config.toml` table is:

```toml
[relay]
enabled = true
api_url = "https://api.stackspirit.io"
relay_url = "wss://relay.stackspirit.io/v1/machine"
key_file = "relay.key"
credential_file = "relay.json"
project = "vowp"
allow_http_read = true
allow_http_write = false
allow_mcp_read = true
allow_mcp_write = false
allow_ssh = false
allow_exec = false
ssh_address = "127.0.0.1:22"
ssh_user = "stackspirit"
# daemon_url = "http://127.0.0.1:4000/"   # empty means read <home>/web-window
max_channels = 64
heartbeat_secs = 20
reconnect_min_secs = 1
reconnect_max_secs = 60
```

`NodeFile` is `#[serde(deny_unknown_fields)]`, so this table must be added to the struct in the same
slice as the config test. The booleans are the machine's local policy; a ticket can never enable a
scope that is false here. `enabled = false` is the default and makes `stackspirit relay run` exit
with a clear "relay is disabled" message. `stackspirit relay enroll` writes `enabled = true` and
preserves every other user setting.

#### Enrollment sequence

```text
Owner (app or CLI, logged in)
  ── POST /v1/machines/enroll-tokens {name, ttl_secs: 900}
  ◄── {token: "ssm_...", expires_at}
Owner gives the token to the machine over a trusted channel
  (cloud-init user-data with a short TTL, or the provider console)

Machine
  ── POST /v1/machines/enroll
     {token, public_key, key_id, name, os, arch, agent_version, capabilities}
  ◄── {machine_id, account_id, machine_credential, relay_url, api_url, expires_at}
  writes relay.key (0600), relay.json (0600), config.toml [relay]
```

The token is `ssm_` plus 32 random bytes in base64url (43 characters). The API stores only its
SHA-256 hash, the account, the requested name, the expiry and a `used_at` field. It is single-use and
expires after at most 3600 seconds. A duplicate enrollment response due to a lost network reply is
handled with an `Idempotency-Key` header: the same token and key return the same machine record; a
different key is `409 conflict`. The token, its hash and the signature are never logged.

#### Machine credential

The machine credential is an ES256 JWT:

```json
{
  "iss": "https://api.stackspirit.io",
  "aud": "relay.stackspirit.io",
  "sub": "machine:mch_01JQ8Z6K4W5N7R9T2V3X4Y5Z6A",
  "acct": "acct_01JQ8Z6K4W5N7R9T2V3X4Y5Z6A",
  "key_id": "ed25519:<hex>",
  "mkey": "<machine Ed25519 public key, lowercase hex>",
  "scopes": ["machine.connect"],
  "iat": 1764072000,
  "exp": 1764158400,
  "jti": "jti_01JQ8Z..."
}
```

The relay verifies the JWT with the API JWKS, checks `aud`, `sub`, `acct`, `key_id` and `mkey`, and
then challenges the machine. The credential is a bearer token only for **authorisation to attempt**
the challenge; it cannot sign the challenge, so a stolen credential cannot impersonate the machine.
The machine refreshes it before `exp` with a signed request:

```text
POST /v1/machines/{machine_id}/credential
Content-Type: application/json

{"key_id":"ed25519:<hex>","signed_at":"2026-11-25T12:00:00.000Z","signature":"<hex>"}

canonical = "STACKSPIRIT-CREDENTIAL-V1\n" + machine_id + "\n" + key_id + "\n" + signed_at
```

The API accepts a timestamp within 60 seconds, verifies the signature with the registered public key
and returns a fresh credential. A revoked machine gets `403` and the connector stops.

Both the relay and the machine cache the API JWKS. The relay caches it in memory for at most 15
minutes; the machine caches it in memory and in `<home>/relay-jwks.json` (0644, no secret) for at
most 24 hours. A ticket that cannot be verified against a fresh or cached key is rejected; there is
no "trust any signing key" fallback. The API publishes the current and previous `kid` during a
rotation overlap of 24 hours.

#### Key rotation

`stackspirit relay rotate-key` generates a new Ed25519 key, signs a rotation request with the old
key, calls `POST /v1/machines/{id}/keys`, writes the new `relay.key` atomically and updates
`relay.json`. The old key remains valid for 24 hours to drain connections, then is revoked. Clients
that cached the old machine key refetch the machine card; a ticket minted with the old `mkey` fails
the client's signature check, so rotation is visible and never silent.

### 6.3 Relay control protocol

**Transport.** `wss://relay.stackspirit.io/v1/machine` and
`wss://relay.stackspirit.io/v1/client`; TLS 1.3; subprotocol `stackspirit-relay/1`; one WebSocket
**text** message carries exactly one JSON `RelayFrame`; one **binary** message carries exactly one
`RelayDataFrame` (encrypted). Compression is not offered. No query-string credential, cookie or
trusted header is accepted. A frame over 1,048,576 bytes closes with WebSocket code `1009`.

**Control frames.** All control frames carry `v: 1` and a `type` tag:

```rust
#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RelayFrame {
    MachineHello {
        v: u8,
        machine_id: String,
        account_id: String,
        key_id: String,
        public_key: String,
        credential: String,
        agent_version: String,
        capabilities: MachineCapabilities,
        sent_at: Timestamp,
    },
    RelayChallenge {
        v: u8,
        relay_id: String,
        session_id: String,
        nonce: String,
        server_time: Timestamp,
        heartbeat_secs: u64,
        max_frame_bytes: u64,
        features: Vec<String>,
    },
    MachineAuth { v: u8, signature: String },
    RelayWelcome {
        v: u8,
        relay_id: String,
        session_id: String,
        machine_id: String,
        account_id: String,
        heartbeat_secs: u64,
        max_frame_bytes: u64,
        features: Vec<String>,
    },
    MachineStatus {
        v: u8,
        state: MachineState,
        channels: u32,
        sent_at: Timestamp,
    },
    ClientHello {
        v: u8,
        machine_id: String,
        ticket: String,
        client_id: String,
        kind: ClientKind,
        protocol: String,
        capabilities: Vec<String>,
        client_key: Jwk,
        client_nonce: String,
        sent_at: Timestamp,
    },
    ClientWelcome {
        v: u8,
        relay_id: String,
        session_id: String,
        channel_id: String,
        machine_id: String,
        account_id: String,
        expires_at: Timestamp,
        features: Vec<String>,
    },
    ChannelOpen {
        v: u8,
        channel_id: String,
        machine_id: String,
        client_id: String,
        kind: ClientKind,
        ticket: String,
        scopes: Vec<Scope>,
        client_key: Jwk,
        client_nonce: String,
    },
    ChannelAccept {
        v: u8,
        channel_id: String,
        machine_key: Jwk,
        machine_nonce: String,
        machine_key_id: String,
        machine_signature: String,
        suite: String,
    },
    ChannelReject {
        v: u8,
        channel_id: String,
        code: String,
        message: String,
        retryable: bool,
    },
    ChannelClose {
        v: u8,
        channel_id: String,
        code: String,
        message: String,
    },
    Superseded { v: u8, reason: String },
    Ping { v: u8, nonce: String },
    Pong { v: u8, nonce: String },
    Error {
        v: u8,
        code: String,
        message: String,
        retryable: bool,
    },
}
```

`MachineCapabilities`:

```json
{
  "protocol": "stackspirit-remote/1",
  "http": true,
  "mcp": true,
  "ssh": false,
  "exec": false,
  "max_channels": 64,
  "projects": ["vowp"]
}
```

`MachineState` is `online`, `degraded`, `offline` or `revoked`. `ClientKind` is `web`, `cli`,
`desktop` or `mcp`. In `channel.accept`, `machine_key` is the **ephemeral P-256** key used for
ECDH; `machine_key_id` is the machine's long-lived Ed25519 identity id and must equal the ticket's
`mkey_id`. The client verifies `machine_signature` with the ticket's `mkey`, never with
`machine_key`.

A control message over 65,536 bytes closes with `1009`. Readers reject an unknown `type` and an
unsupported `v`; they ignore unknown fields so a future optional field is additive. A missing
required field is a protocol error.

**Machine authentication.** The canonical string is exactly:

```text
STACKSPIRIT-RELAY-AUTH-V1
<machine_id>
<account_id>
<key_id>
<relay_id>
<session_id>
<nonce>
<server_time>
```

There is one `\n` between fields and **no trailing newline**. `signature` is the lowercase hex
Ed25519 signature over the UTF-8 string. `server_time` is the exact RFC 3339 millisecond string from
`relay.challenge`. The relay verifies it with `mkey` from the credential; a bad signature gets
`Error{code:"unauthenticated"}` and close code `4001`. A nonce is single-use per relay session; a
replay gets `unauthenticated` and close `4001`.

**Client authentication.** The first client text frame must be `client.hello` within 5 seconds; it
carries the ticket, the client's ephemeral P-256 `client_key` (JWK) and a 32-byte `client_nonce`.
The relay verifies the ticket (see §6.4), checks that the machine is online and that every requested
scope is a subset of the ticket, and may check `cnf.jkt` against `client_key`. It then allocates a
`channel_id` and sends `client.welcome`. It also sends `channel.open` to the machine with the same
ticket, `client_key` and `client_nonce`.

**WebSocket close codes.**

| Code | Meaning |
|---|---|
| `1000` | normal close |
| `1009` | frame over the cap |
| `1013` | backpressure or temporary overload |
| `4001` | authentication failed |
| `4004` | machine offline |
| `4008` | superseded by another connection |
| `4009` | rate limited |
| `4010` | ticket expired |
| `4011` | scope denied |
| `4012` | E2E handshake or crypto failure |

A `channel.close` control frame is an operational signal from the relay (machine offline, timeout,
backpressure). A client treats it as a transport failure, not as a trusted application-level close;
an authenticated application close travels encrypted as `RemoteFrame::Close`.

**Limits.** Machine connections: one per machine; a second closes the first with `superseded`.
Clients: 32 per machine. Channels: 64 per machine (configurable to 128). Handshake timeout: 5
seconds. Heartbeat: relay `Ping` every 20 seconds; no `Pong` for 60 seconds closes. Idle channel:
15 minutes without a data frame for HTTP SSE, 60 seconds otherwise. Machine egress: 16 MiB/s;
client egress: 8 MiB/s.

### 6.4 API and tickets

The API is a Cloudflare Worker with D1. Its public base is `https://api.stackspirit.io/v1`.

#### Account authentication

- **Browser:** OIDC authorisation code with PKCE; an HttpOnly, Secure, SameSite=Lax
  `__Host-ss_session` cookie; a CSRF token for writes. The access token never enters localStorage.
- **CLI:** device authorisation grant. `POST /v1/auth/device` returns a device code, user code and
  verification URL; `POST /v1/auth/device/token` returns an access token (15 minutes) and a refresh
  token (30 days). The refresh token is stored in the OS keyring; the fallback file is
  `~/.config/stackspirit/credentials.json` at mode 0600. A command-line flag never carries a token.
- **Hosted MCP:** OAuth 2.1 authorization server with PKCE and dynamic client registration. The
  MCP access token has audience `https://mcp.stackspirit.io`, scopes `mcp.read` and `mcp.write`, and
  a 15-minute lifetime.

#### Endpoints

| Method | Path | Auth | Purpose |
|---|---|---|---|
| `POST` | `/v1/auth/device` | none | start a CLI device flow |
| `POST` | `/v1/auth/device/token` | device code | exchange for access/refresh tokens |
| `POST` | `/v1/auth/refresh` | refresh token | rotate the access token |
| `GET` | `/v1/me` | access token | account profile |
| `POST` | `/v1/machines/enroll-tokens` | account | create a one-time machine token |
| `POST` | `/v1/machines/enroll` | enrollment token | bind a machine public key |
| `GET` | `/v1/machines` | account | machines and presence |
| `GET` | `/v1/machines/{id}` | account | machine card and public key |
| `DELETE` | `/v1/machines/{id}` | account | revoke a machine |
| `POST` | `/v1/machines/{id}/credential` | machine signature | refresh a machine credential |
| `POST` | `/v1/machines/{id}/keys` | machine signature | rotate the machine key |
| `POST` | `/v1/relay-tickets` | account | mint a short-lived relay ticket |
| `GET` | `/v1/audit` | account | audit rows, never payloads |
| `GET` | `/.well-known/jwks.json` | none | ES256 signing keys |
| `GET` | `/.well-known/oauth-authorization-server` | none | hosted MCP OAuth metadata |
| `GET` | `/.well-known/oauth-protected-resource` | none | hosted MCP resource metadata |
| `GET` | `/v1/health` | none | liveness |

Ticket request and response:

```json
POST /v1/relay-tickets
Authorization: Bearer <account access token>
Content-Type: application/json

{
  "machine_id": "mch_01JQ8Z6K4W5N7R9T2V3X4Y5Z6A",
  "kind": "web",
  "scopes": ["http.read", "mcp.read"],
  "client_key": {"kty": "EC", "crv": "P-256", "x": "<base64url>", "y": "<base64url>"},
  "ttl_secs": 300
}
```

```json
{
  "ticket": "<ES256 JWT>",
  "expires_at": "2026-11-25T12:05:00.000Z",
  "relay_url": "wss://relay.stackspirit.io/v1/client",
  "machine": {
    "id": "mch_01JQ8Z6K4W5N7R9T2V3X4Y5Z6A",
    "name": "contabo-1",
    "key_id": "ed25519:<hex>",
    "public_key": "<hex>",
    "project": "vowp"
  }
}
```

Ticket claims:

```json
{
  "iss": "https://api.stackspirit.io",
  "aud": "relay.stackspirit.io",
  "sub": "client:cli_01JQ8Z...",
  "acct": "acct_01JQ8Z...",
  "machine": "mch_01JQ8Z...",
  "kind": "web",
  "scopes": ["http.read", "mcp.read"],
  "cnf": {"jkt": "<RFC 7638 SHA-256 thumbprint of client_key>"},
  "mkey": "<machine Ed25519 public key, lowercase hex>",
  "mkey_id": "ed25519:<hex>",
  "iat": 1764072000,
  "exp": 1764072300,
  "jti": "jti_01JQ8Z..."
}
```

The ticket lifetime is at most 300 seconds. The API clamps the requested scopes to the intersection
of the account's grant, the machine's allowed scopes and the client kind. The relay verifies the
ticket and forwards it to the machine; the machine re-verifies it and checks `cnf` against the
client's ephemeral key. A ticket is single-use within its lifetime: the `jti` is cached by the
relay and the machine for 600 seconds. A second use gets `ticket_replayed`.

#### D1 tables

```sql
CREATE TABLE accounts (
  id TEXT PRIMARY KEY, email TEXT NOT NULL, created_at TEXT NOT NULL
);
CREATE TABLE machines (
  id TEXT PRIMARY KEY, account_id TEXT NOT NULL, name TEXT NOT NULL,
  public_key TEXT NOT NULL, key_id TEXT NOT NULL, status TEXT NOT NULL,
  project TEXT, ssh_user TEXT, created_at TEXT NOT NULL,
  last_seen_at TEXT, revoked_at TEXT
);
CREATE TABLE enroll_tokens (
  token_hash TEXT PRIMARY KEY, account_id TEXT NOT NULL, name TEXT NOT NULL,
  expires_at TEXT NOT NULL, used_at TEXT
);
CREATE TABLE machine_keys (
  machine_id TEXT NOT NULL, key_id TEXT NOT NULL, public_key TEXT NOT NULL,
  created_at TEXT NOT NULL, revoked_at TEXT, PRIMARY KEY (machine_id, key_id)
);
CREATE TABLE audit (
  id TEXT PRIMARY KEY, account_id TEXT NOT NULL, machine_id TEXT,
  actor TEXT NOT NULL, action TEXT NOT NULL, at TEXT NOT NULL,
  detail_json TEXT NOT NULL
);
```

There is no payload table, no prompt table and no private-key column. The API stores no SSH key.
`audit.detail_json` records ids and outcomes only; it never records a ticket, a signature, a token,
a prompt or tool output. Enrollment tokens are deleted 24 hours after expiry; revoked machines are
retained 90 days for audit.

### 6.5 End-to-end channel

The channel protects payloads from the relay, Cloudflare and any passive network observer. The
client is authenticated by the API ticket; the machine is authenticated by its Ed25519 identity.
The key exchange is ephemeral P-256 ECDH with HKDF-SHA256 and AES-256-GCM. P-256, HKDF and AES-GCM
are available in Rust (`p256`, `hkdf`, `aes-gcm`), in Cloudflare Workers and in browser WebCrypto,
so one implementation is testable across all three.

#### Handshake

1. The client generates an ephemeral P-256 key pair `(c_priv, c_pub)` and a 32-byte `client_nonce`
   before requesting the ticket. `cnf.jkt` is the RFC 7638 thumbprint of `c_pub` in canonical JWK
   form. The client keeps `c_priv` in memory only.
2. The relay sends `channel.open` to the machine with the ticket, `client_key` (JWK) and
   `client_nonce`.
3. The machine verifies the ticket signature, `aud`, `exp`, `machine`, `acct`, `scopes` and `jti`,
   and checks `cnf.jkt` against `client_key`. It rejects a key substitution, an expired ticket, a
   replayed `jti`, or a scope not enabled in local policy.
4. The machine generates an ephemeral P-256 key pair `(m_priv, m_pub)` and a 32-byte
   `machine_nonce`, then builds the transcript:

   ```text
   T = UTF8("stackspirit-e2e-v1\n" +
            channel_id + "\n" +
            canonical_jwk(c_pub) + "\n" +
            canonical_jwk(m_pub) + "\n" +
            client_nonce + "\n" +
            machine_nonce + "\n" +
            machine_id + "\n" +
            account_id + "\n" +
            ticket_jti)
   ```

   `canonical_jwk` is the RFC 7638 canonical JSON with lexicographic keys and no whitespace:
   `{"crv":"P-256","kty":"EC","x":"...","y":"..."}`. The machine signs `T` with its Ed25519
   identity and sends `channel.accept` with `machine_key` (the ephemeral `m_pub`), `machine_nonce`,
   `machine_key_id` and `machine_signature`.
5. The client verifies `machine_signature` with the `mkey` in its ticket **before** deriving keys.
   A mismatch aborts the channel with `crypto_error`.
6. Both sides compute:

   ```text
   Z      = P-256 ECDH(c_priv, m_pub) = P-256 ECDH(m_priv, c_pub)   // 32-byte x
   salt   = SHA-256(T)
   prk    = HKDF-Extract(salt, Z)
   c2m    = HKDF-Expand(prk, "stackspirit c2m key" || 0x01, 32)
   m2c    = HKDF-Expand(prk, "stackspirit m2c key" || 0x01, 32)
   c2m_iv = HKDF-Expand(prk, "stackspirit c2m iv", 4)
   m2c_iv = HKDF-Expand(prk, "stackspirit m2c iv", 4)
   ```

   `0x01` is a one-byte domain-separation label; the info strings are UTF-8 and concatenated
   without a separator. An all-zero ECDH result or an invalid point aborts the channel.
7. Each direction has its own key and sequence. The client-to-machine direction uses `c2m` /
   `c2m_iv`; the machine-to-client direction uses `m2c` / `m2c_iv`.

#### Data frame

`channel_id` is a 26-character ULID string in control frames and the 16 raw ULID bytes in a data
frame. It is allocated by the relay for one client/machine channel and never reused.

One binary WebSocket message:

| Offset | Size | Field |
|---|---|---|
| 0 | 4 | magic `SSR1` (`0x53 0x53 0x52 0x31`) |
| 4 | 1 | version `0x01` |
| 5 | 1 | type `0x01` (data); `0x02` is reserved for rekey and is rejected in v1 |
| 6 | 2 | flags, big-endian, zero in v1 |
| 8 | 16 | channel id, the raw ULID bytes |
| 24 | 8 | sequence, big-endian `u64`, starts at 1 |
| 32 | 4 | ciphertext length, big-endian `u32` |
| 36 | N | AES-256-GCM ciphertext including the 16-byte tag |

The nonce is `c2m_iv || seq` (4 + 8 = 12 bytes) in the sending direction. The additional
authenticated data is bytes 0..32 of the frame, so the header cannot be altered. A receiver rejects
a sequence that is not exactly one greater than the last accepted sequence for that direction, a
ciphertext that fails authentication, a frame over 1,048,576 bytes, or an unknown channel id. A
failure closes the channel with `crypto_error`; there is no plaintext fallback.

The inner plaintext is one UTF-8 JSON `RemoteFrame` (§6.6), at most 768 KiB. A larger HTTP body is
chunked into multiple `HttpBody` frames. The outer frame cap and the inner cap are both enforced;
a sender never sends a partial frame.

#### Why the relay cannot read

The relay sees `channel.open` and `channel.accept` (tickets, public keys, nonces, signatures) and
the binary frames. It does not see `c_priv` or `m_priv`, so it cannot compute `Z`. Without `Z` it
cannot derive `c2m`/`m2c`; without those keys it cannot decrypt or forge a frame. A relay that steals
a ticket still lacks the client's ephemeral private key, so it cannot complete the handshake as that
client. A relay that substitutes its own machine key fails the client's `mkey` signature check.

### 6.6 Remote frames and channels

The E2E plaintext is a JSON `RemoteFrame`:

```rust
#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RemoteFrame {
    Hello { v: u8, client: String, machine: String, scopes: Vec<Scope>, capabilities: Vec<String> },
    HelloAck { v: u8, machine: String, scopes: Vec<Scope>, capabilities: Vec<String> },
    HttpRequest { stream: u32, method: String, path: String, headers: BTreeMap<String, String>, body: String },
    HttpResponse { stream: u32, status: u16, headers: BTreeMap<String, String> },
    HttpBody { stream: u32, data: String, end: bool },
    HttpCancel { stream: u32 },
    McpRequest { stream: u32, project: Option<String>, session: Option<String>, request: serde_json::Value },
    McpResponse { stream: u32, response: serde_json::Value },
    McpCancel { stream: u32 },
    TcpOpen { stream: u32, target: String },
    TcpData { stream: u32, data: String },
    TcpClose { stream: u32 },
    ExecStart { stream: u32, argv: Vec<String>, cwd: Option<String>, pty: bool, cols: u16, rows: u16, env: BTreeMap<String, String> },
    ExecData { stream: u32, data: String, channel: ExecChannel },
    ExecResize { stream: u32, cols: u16, rows: u16 },
    ExecExit { stream: u32, code: i32, signal: Option<String> },
    StreamError { stream: u32, code: String, message: String, retryable: bool },
    Ping { nonce: String },
    Pong { nonce: String },
    Close { code: String, message: String },
}
```

The first encrypted frame from the client is `Hello`; the machine answers with `HelloAck` carrying
the granted scopes and capabilities. The machine must reject a requested scope it did not enable
before it serves the first stream. `HelloAck` is the authoritative grant; the ticket is the
authorisation input, not the final grant.

`stream` is a client-allocated `u32` in `1..=2_147_483_647`; `0` is reserved. `data` and `body`
are standard base64 with padding; a chunk is at most 512 KiB encoded. `Scope` is one of
`http.read`, `http.write`, `mcp.read`, `mcp.write`, `ssh`, `exec`. `ExecChannel` is `stdout`,
`stderr` or `stdin`. A `StreamError.message` is path-free and redacted; it never contains a secret,
a prompt, a tool argument or an absolute path.

#### HTTP channel

`HttpRequest.path` must be an absolute path under `/api/v1/`; it is normalised before use:

- reject `..`, `//`, `\`, `%2f`, `%5c`, NUL, CR/LF and an encoded path separator;
- reject a query over 2 KiB or a query key named `token`, `key`, `secret`, `auth` or `code`;
- allow only `GET` and `POST`; a POST path must be one of the explicit write routes below;
- allow request headers `Accept`, `Content-Type`, `Last-Event-ID` and `If-None-Match`; reject
  `Authorization`, `Cookie`, `Host` and every `X-Forwarded-*` header;
- allow response headers `Content-Type`, `Cache-Control`, `ETag`, `Last-Modified`, `Location` and
  `Retry-After`; never forward `Set-Cookie` or `WWW-Authenticate`.

The machine adds its own local `Authorization: Bearer <VOWP_API_TOKEN>` for write requests; the
client never receives that token. If the local daemon has no write token, the request gets the
daemon's existing `503` and no write occurs.

The connector itself does not need the token for read-only v1. If the owner enables remote writes,
`stackspirit up` and `stackspirit relay run` must receive the same `VOWP_API_TOKEN`. The systemd
templates use an optional `/etc/stackspirit/daemon.env` file at mode 0600 for this; the value is
never written to `config.toml`, `relay.json`, a command line or a log. A connector without the
token still serves reads, and every write returns the daemon's `503` unchanged.

v1 write routes:

```text
POST /api/v1/inbox/{id}/answer
POST /api/v1/sessions/{id}/switch
POST /api/v1/sessions/import/asif
POST /api/v1/jobs/{id}/retry
POST /api/v1/jobs/{id}/park
POST /api/v1/jobs/{id}/cancel
POST /api/v1/jobs/{id}/nudge
POST /api/v1/jobs/{id}/restart
POST /api/v1/jobs/{id}/replan
POST /api/v1/jobs/{id}/fail
```

The read allowlist is the existing `/api/v1` surface: health, board and board stream, jobs, job
detail/runs/gates, sessions, session detail/events/stream, inbox and stream, nodes and stream, cost
and stream. Static pages, `sw.js`, the manifest and the icon are served by `app.stackspirit.io`, not
tunnelled.

`HttpResponse` starts a response; `HttpBody` chunks follow with `end: true` on the last chunk. The
stream is bounded to 8 MiB for a non-SSE response and one hour for an SSE response. `HttpCancel`
cancels the local request; the machine drops the response stream. A 4xx/5xx status is a normal
`HttpResponse`, not an `StreamError`, except for a transport failure.

#### MCP channel

`McpRequest.request` is one complete JSON-RPC 2.0 object (no batch array). The hosted MCP server
maps MCP methods to HTTP requests in v1, so the machine does not need a second MCP parser:

| MCP tool | HTTP mapping | Scope |
|---|---|---|
| `fleet_status` | `GET /api/v1/board` | `mcp.read` |
| `fleet_queue` | `GET /api/v1/jobs?state=ready` | `mcp.read` |
| `inbox_list` | `GET /api/v1/inbox?state=open` | `mcp.read` |
| `capacity_report` | `GET /api/v1/nodes` | `mcp.read` |
| `session_tail` | `GET /api/v1/sessions/{id}/events?limit=...` | `mcp.read` |
| `inbox_answer` | `POST /api/v1/inbox/{id}/answer` | `mcp.write` |
| `job_pause`, `job_retry` | the matching job POST route | `mcp.write` |

The remaining MCP tools (`job_add`, `capacity_set`, `tunnel_open`, `tunnel_close`, `secret_request`,
`session_handoff`, `session_resume`) are **not** advertised by the hosted MCP server in v1; they
need a session lease or a route that does not exist. A future machine-side MCP channel can add them
without changing the relay framing. The tool names that are advertised stay exactly the existing
names; no prefix is added.

`initialize` returns `serverInfo.name: "stackspirit-engine"` and `capabilities.tools`. `tools/list`
omits a write tool when the ticket lacks `mcp.write`; the machine repeats the filter. `tools/call`
for an omitted tool returns a JSON-RPC error with code `-32601` and a message naming the missing
scope. `ping` is answered locally. `McpCancel` maps to a cancelled request; a call timeout is 120
seconds.

#### TCP/SSH channel

`TcpOpen.target` is the literal string `ssh` in v1. The machine resolves it to
`[relay].ssh_address` (default `127.0.0.1:22`) and connects. The client cannot choose a host, port
or arbitrary target. `TcpData` carries raw bytes (base64); the machine writes them to the socket and
returns bytes in the other direction. `TcpClose` closes the socket. The channel is allowed only
when the machine enabled `[relay].allow_ssh = true` and the ticket has `ssh`.

`stackspirit ssh` uses this channel with the system OpenSSH client:

```sh
stackspirit ssh <machine> [-- <ssh-args>...]

# The command resolves the machine, gets an `ssh` ticket, and runs:
ssh -o "ProxyCommand=<stackspirit> relay proxy --machine <machine_id> --stdio" \
    -o "HostKeyAlias=stackspirit-<machine_id>" \
    <ssh_user>@stackspirit-<machine_id> <ssh-args>...
```

`stackspirit relay proxy` is a hidden command used only as the `ProxyCommand`. It opens the relay
client, completes the E2E handshake, opens one `tcp` stream and copies stdin/stdout raw; it never
parses SSH. The user's SSH key and the machine's host key stay between the OpenSSH client and
`sshd`. The relay sees only AES-GCM ciphertext containing SSH ciphertext. The machine's `sshd` can
listen on loopback; no public SSH port is required. A machine without `sshd` returns
`StreamError{code:"ssh_unavailable"}`; `stackspirit ssh --exec` (planned) would run the machine's
`stackspirit` CLI in a PTY instead, and is not part of v1.

#### Exec channel

`ExecStart` is defined for a future `stackspirit exec`; it is disabled by default and not used by
`stackspirit ssh`. When enabled, `[relay].exec_allow` must contain the absolute `argv[0]`, `cwd`
must be under the home, `env` is limited to `TERM`, `LANG`, `LC_ALL`, `TZ` and `STACKSPIRIT_*`
names without secret values, and the process runs as the same user as the connector. The scope is
`exec`. A request outside the allowlist returns `exec_denied`. No shell is implied unless
`[relay].exec_shell = true`, which is a separate owner decision.

### 6.7 Clients

#### Web/PWA at `app.stackspirit.io`

- Static Cloudflare Pages app. Login through the API OIDC flow; the session cookie is HttpOnly.
- Machine picker from `GET /v1/machines`; each card shows online/offline and the project.
- On selection, the app requests a ticket with `kind: "web"`, scopes `http.read` and (if enabled)
  `http.write`, and an ephemeral P-256 key from WebCrypto. It keeps the private key in memory.
- It opens one relay client connection and one E2E channel, then issues `HttpRequest` frames for the
  existing `/api/v1` pages: board, jobs, sessions, inbox, nodes, cost, with SSE for the streams.
- The UI reuses the existing daemon page contracts; it does not reimplement the read model. A
  machine that is offline shows the cached board only if the browser has one; no server-side cache
  exists.
- The ticket is in memory only. A page reload requests a new ticket; the ephemeral key is new, so a
  stolen old ticket cannot be reused.
- The web app never stores the machine public key as a trust anchor beyond the ticket's `mkey`; a
  changed key is a hard abort with a "machine key changed" message.

#### Hosted remote MCP at `mcp.stackspirit.io`

- Streamable HTTP MCP endpoint at `/mcp`, JSON-RPC 2.0. OAuth 2.1 resource server with PKCE;
  `WWW-Authenticate` points at the protected-resource metadata.
- `GET /.well-known/oauth-protected-resource`:
  ```json
  {
    "resource": "https://mcp.stackspirit.io",
    "authorization_servers": ["https://api.stackspirit.io"],
    "scopes_supported": ["mcp.read", "mcp.write"],
    "bearer_methods_supported": ["header"]
  }
  ```
- On `initialize`, return `serverInfo.name: "stackspirit-engine"`, `protocolVersion` from the
  request, and `capabilities.tools`.
- On `tools/list`, return only the v1 tools in §6.6, filtered by the ticket scopes.
- On `tools/call`, get a ticket with `kind: "mcp"` and the requested scope, open or reuse a pooled
  channel for 60 seconds, send `McpRequest`, wait at most 120 seconds, and return the result. A
  tool result is never logged or cached after the response.
- A write tool requires both the OAuth scope and the machine's `mcp.write` policy; otherwise the
  response is a JSON-RPC error and the machine records an audit row.

#### Desktop

StackSpirit Desktop embeds the same TypeScript remote client as the PWA (or calls the CLI for
`ssh`). It uses the same API endpoints, ticket shape, E2E handshake and HTTP/MCP frames. There is no
desktop-only wire protocol. The desktop may pin the machine public key in its local preferences; a
change requires an explicit trust action.

#### CLI and `stackspirit ssh`

New commands (all planned until their slice lands):

```text
stackspirit login [--api https://api.stackspirit.io]
stackspirit logout
stackspirit machine list|show|revoke
stackspirit relay enroll --token-file - [--name <name>] [--project <name>] [--ssh-user <user>]
stackspirit relay status [--check] [--json]
stackspirit relay run [--service]
stackspirit relay rotate-key
stackspirit ssh <machine> [-- <ssh-args>...]
stackspirit relay proxy --machine <machine> --stdio   # hidden
stackspirit mcp --remote <machine>                   # planned, hosted MCP over the relay
```

`stackspirit mcp` without `--remote` remains the local stdio MCP server; with no flags it uses the
read-only mode from §3.7, so `npx -y stackspirit mcp` works without an account. `stackspirit ssh`
requires the local OpenSSH client and a machine that permits `ssh`; it is not a second SSH
implementation and not a root shell. The command uses the existing exit-code classes; auth errors
are `CliError::Auth` and connect errors are `CliError::Remote`.

### 6.8 Relay Worker and Durable Object

`cloudflare/relay/src/index.ts` routes:

| Route | Behaviour |
|---|---|
| `GET /v1/machine` | WebSocket upgrade only; first frame `machine.hello` within 5 s |
| `GET /v1/client` | WebSocket upgrade only; first frame `client.hello` within 5 s |
| `GET /v1/health` | `{"ok":true,"protocol":"stackspirit-relay/1"}` |
| `GET /v1/version` | supported protocol and feature list |
| anything else | `404`; a non-upgrade `GET` on a WebSocket route is `426`; a wrong subprotocol is `400` |

Each machine id maps to one Durable Object `MachineHub`. The object:

- accepts the machine socket with the WebSocket hibernation API and stores only connection metadata
  (`machine_id`, `account_id`, `session_id`, `last_seen`, channel ids) in memory/DO storage;
- validates the machine credential against cached API JWKS and sends `relay.challenge`;
- accepts clients, validates tickets, checks scope subsets and machine presence, and creates a
  `channel_id`;
- forwards `channel.open`/`channel.accept` and binary frames without inspecting plaintext;
- enforces frame caps, channel caps, backpressure and idle timeouts;
- closes all clients with `machine_offline` when the machine socket closes, and closes the old
  machine socket with `superseded` when a new one connects;
- records presence through the API service binding (`POST /v1/internal/presence`) with a service
  token; the public API has no internal route;
- never writes a payload, prompt, ticket, signature or SSH byte to DO storage or Workers Logs.

Backpressure: if a peer's `bufferedAmount` exceeds 1 MiB, the relay drops new data frames for that
channel and sends `Error{code:"rate_limited"}`; if it stays over the limit for 10 seconds, it closes
with `1013`. It never buffers unbounded payloads.

Revocation: on `machine.hello`, the relay checks the machine status through the API service binding
with a 60-second cache. A revoked machine gets `Error{code:"forbidden"}` and close `4001`; its
clients get `machine_offline`. A revoked account's tickets fail validation within the ticket TTL.

### 6.9 Presence, reconnect and resume

- The machine connector reconnects with exponential backoff from 1 s to 60 s plus 0–250 ms jitter;
  a stable 60-second connection resets the backoff.
- `Resume` is not needed at the relay layer: a channel is live-only. A dropped channel is a failed
  request; the client retries a **read** or an idempotent request. Writes are not auto-retried
  unless the underlying HTTP operation is explicitly idempotent. This avoids duplicate job actions.
- The machine keeps local state; a relay outage does not stop the supervisor, jobs or sessions. The
  connector reports `offline` in `stackspirit relay status` and reconnects.
- The API presence row is updated by the relay on connect, heartbeat and disconnect. A missing
  heartbeat for 90 seconds marks the machine `offline`; a revoked machine is `revoked` regardless.
- The connector refreshes its credential when less than six hours remain; a failed refresh logs a
  path-free reason and retries with backoff while the current credential is still valid. An expired
  credential fails closed.

### 6.10 Iroh hook

The protocol carries transport hints so a future direct P2P path does not change the application
framing:

```json
{
  "transports": [
    {"kind": "relay", "url": "wss://relay.stackspirit.io/v1/client", "priority": 0},
    {"kind": "iroh", "node_id": "<base32>", "addrs": ["udp://<public-ip>:4433"], "priority": 1}
  ]
}
```

The API machine card and `relay.welcome` include the same list. A client tries a higher-priority
transport with a 3-second timeout, then falls back to the relay. The `RemoteFrame` and E2E handshake
are transport-independent; Iroh carries the same `RelayDataFrame` bytes after a short stream
handshake. No Iroh dependency is added in this design; the hook is the `transports` field plus a
`RemoteTransport` trait in `vowp-agent`. Turning Iroh on is a later owner decision and a separate
slice.

## 7. Contabo enrollment and the optional R2 backup

### 7.1 Supported shape

- Ubuntu 24.04 LTS, x86_64 or arm64, systemd, a non-root `stackspirit` user.
- Cloud-init user-data is written by the owner in the Contabo console; the repository contains the
  template, not a token.
- The machine dials out to `api.` and `relay.stackspirit.io` on 443. It opens no public port for the
  relay. SSH remains available for bootstrap and, if the owner chooses, through `stackspirit ssh`
  over the relay.

### 7.2 Cloud-init flow

```yaml
#cloud-config
users:
  - name: stackspirit
    groups: [sudo]
    home: /var/lib/stackspirit
    shell: /bin/bash
    sudo: ["ALL=(ALL) NOPASSWD:/bin/systemctl restart stackspirit.service, /bin/systemctl restart stackspirit-relay.service"]
    ssh_authorized_keys:
      - __OWNER_SSH_PUBLIC_KEY__
package_update: true
packages: [ca-certificates, curl, git, tmux, ufw, openssh-server]
write_files:
  - path: /etc/stackspirit/enroll.token
    owner: root:root
    permissions: "0600"
    content: |
      __ONE_TIME_ENROLL_TOKEN__
  - path: /etc/stackspirit/cloud-init.env
    owner: root:root
    permissions: "0600"
    content: |
      STACKSPIRIT_VERSION=__VERSION__
      STACKSPIRIT_PROJECT=__PROJECT__
      STACKSPIRIT_RELAY_NAME=__HOSTNAME__
runcmd:
  - [bash, -c, "ufw --force reset"]
  - [bash, -c, "ufw default deny incoming; ufw default allow outgoing; ufw allow 22/tcp; ufw deny out 25/tcp; ufw deny out 465/tcp; ufw deny out 587/tcp; ufw --force enable"]
  - [bash, -c, "curl -fsSL https://get.stackspirit.io/install.sh -o /tmp/install.sh"]
  - [bash, -c, "curl -fsSL https://get.stackspirit.io/install.sh.sha256 -o /tmp/install.sh.sha256 && cd /tmp && sha256sum -c install.sh.sha256"]
  - [bash, -c, ". /etc/stackspirit/cloud-init.env; bash /tmp/install.sh --system --product stackspirit --version \"$STACKSPIRIT_VERSION\""]
  - [bash, -c, "install -d -o stackspirit -g stackspirit -m 0700 /var/lib/stackspirit/.stackspirit"]
  - [bash, -c, ". /etc/stackspirit/cloud-init.env; sudo -u stackspirit /usr/local/bin/stackspirit relay enroll --api https://api.stackspirit.io --token-file /etc/stackspirit/enroll.token --name \"$STACKSPIRIT_RELAY_NAME\" --project \"$STACKSPIRIT_PROJECT\""]
  - [bash, -c, "rm -f /etc/stackspirit/enroll.token"]
  - [systemctl, enable, --now, stackspirit.service]
  - [systemctl, enable, --now, stackspirit-relay.service]
```

The enroll command writes `relay.key` and `relay.json` as the `stackspirit` user, updates the user's
home `config.toml` with a `[relay]` table, and never logs the token. The cloud-init run is guarded:
if `relay.json` already exists, enrollment is skipped; if the token file is missing, cloud-init
writes a short instruction file and does not start the relay service in a retry loop.

The owner may paste the token through the provider console instead of user-data. A token in
user-data is visible to the provider and anyone who can read the instance metadata; the 15-minute
TTL and single use bound the exposure. The template never contains a real token in this repository.

### 7.3 Firewall and SMTP block

UFW rules are applied before the stackspirit install:

```sh
ufw default deny incoming
ufw default allow outgoing
ufw allow 22/tcp
ufw deny out 25/tcp
ufw deny out 465/tcp
ufw deny out 587/tcp
ufw --force enable
```

This blocks the common SMTP submission and relay ports for all users, including a compromised
harness. Contabo may also block them at the network edge; the host rule is defense in depth. If the
image uses nftables without UFW, the template falls back to equivalent `nft add rule inet filter
output tcp dport {25,465,587} reject` lines; the cloud-init test asserts one of the two forms.

After the owner verifies `stackspirit ssh`, they may remove the public `22/tcp` rule and let `sshd`
listen on loopback only. The relay `tcp` channel then reaches `127.0.0.1:22`. The template does not
remove the rule automatically, because that would lock out an owner who has not yet tested the
relay.

### 7.4 Services

`deploy/systemd/stackspirit.service`:

```ini
[Unit]
Description=StackSpirit supervisor
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=stackspirit
Group=stackspirit
WorkingDirectory=/var/lib/stackspirit
Environment=STACKSPIRIT_HOME=/var/lib/stackspirit/.stackspirit
Environment=VOWP_HOME=/var/lib/stackspirit/.stackspirit
EnvironmentFile=-/etc/stackspirit/daemon.env
ExecStart=/usr/local/bin/stackspirit up --foreground
Restart=on-failure
RestartSec=5
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ProtectHome=read-only
ReadWritePaths=/var/lib/stackspirit/.stackspirit

[Install]
WantedBy=multi-user.target
```

`deploy/systemd/stackspirit-relay.service`:

```ini
[Unit]
Description=StackSpirit outbound relay connector
After=network-online.target stackspirit.service
Wants=network-online.target

[Service]
Type=simple
User=stackspirit
Group=stackspirit
WorkingDirectory=/var/lib/stackspirit
Environment=STACKSPIRIT_HOME=/var/lib/stackspirit/.stackspirit
Environment=VOWP_HOME=/var/lib/stackspirit/.stackspirit
EnvironmentFile=-/etc/stackspirit/daemon.env
ExecStart=/usr/local/bin/stackspirit relay run
Restart=on-failure
RestartSec=5
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ProtectHome=read-only
ReadWritePaths=/var/lib/stackspirit/.stackspirit

[Install]
WantedBy=multi-user.target
```

The relay connector reads `<home>/web-window` to find the loopback daemon started by
`stackspirit up`. If the file is absent, it exits with a clear "start stackspirit up or set
[relay].daemon_url" error; it does not start a second daemon and does not open the database. v1
exposes one selected project per machine; a multi-project connector is a later slice.

### 7.5 Optional encrypted R2 backup

The default is **no backup**. The template installs no timer, no R2 credential and no age key. An
owner who wants offsite backups opts in:

```toml
[backup.r2]
enabled = false
bucket = "stackspirit-backups"
endpoint = "https://<account>.r2.cloudflarestorage.com"
prefix = "machines/<machine-id>"
age_recipient = "age1..."
retain_days = 30
```

`deploy/backup/r2-backup.sh`:

1. Takes a `flock` lock so two runs cannot overlap.
2. Runs `stackspirit backup --json` for a consistent database snapshot.
3. Builds a tar of the home's database, session logs, project configs and selected metadata.
4. Excludes `relay.key`, `relay.pub`, `relay.json`, `node.key`, `secrets/`, `surfaces/`,
   `worktrees/`, `*.lock` and the backup staging directory.
5. Encrypts the tar with `age -r "$age_recipient"` to a temporary `.tar.age`.
6. Uploads with `rclone copyto` to `r2:<bucket>/<prefix>/<timestamp>.tar.age` and writes a
   `.sha256` sidecar.
7. Verifies the remote size and SHA-256, then deletes encrypted objects older than `retain_days`.
8. Writes a status line with the object name and size, never the credentials.

Credentials come from `/etc/stackspirit/rclone.conf` (0600) or systemd `LoadCredential`; the age
identity is kept offline, not on the machine, so a stolen machine cannot decrypt its old backups.
The systemd timer `stackspirit-backup.timer` is installed **disabled**; the owner enables it with
`systemctl enable --now stackspirit-backup.timer`. A restore is a manual `age -d`, untar and
`stackspirit backup --restore` sequence documented in the operator guide.

### 7.6 Failure modes

| Failure | Behaviour |
|---|---|
| Enrollment token expired or used | enroll returns `410`; cloud-init leaves the token file for inspection and does not retry in a loop; owner creates a new token |
| Token visible in user-data | short TTL and single use; the file is removed after enrollment; the owner may rotate the machine key |
| SMTP ports already blocked by the provider | UFW rules still apply; no failure |
| UFW absent | nftables fallback; if neither exists, cloud-init fails before starting the relay and prints the reason |
| `stackspirit up` not running | relay service exits with the daemon hint; it does not create a second daemon |
| Relay unreachable | machine stays local; clients see offline; connector reconnects with backoff |
| R2 credentials missing | backup script exits non-zero before uploading; no partial object |
| age recipient wrong | encryption fails locally; nothing is uploaded |
| Backup includes a private key | the script's exclude list and a test fail the slice |
| No backup configured | no timer, no upload, no warning beyond the operator guide |

## 8. Security threat model

### 8.1 Assets

| Asset | Where it lives | Classification |
|---|---|---|
| Machine private key `relay.key` | machine, 0600 | critical |
| User SSH private keys | owner device / machine `~/.ssh`, never cloud | critical |
| Account refresh token | OS keyring or 0600 file | critical |
| Prompts, tool output, file contents | machine session logs and E2E payloads | confidential |
| Secret values | machine vault / environment | critical |
| Machine credential and relay ticket | machine / client memory | sensitive bearer tokens |
| Public keys, machine ids, presence | API/relay | public/internal |
| Audit rows | API D1 | internal |
| Release artifacts | GitHub, npm, PyPI | public |

### 8.2 Guarantees

1. **No cloud private keys.** The relay and API never receive `relay.key`, an SSH private key, a
   client ephemeral private key or a secret value.
2. **Relay blindness.** The relay cannot decrypt or forge an E2E payload; it sees metadata, frame
   sizes and timing only.
3. **Machine authenticity.** A client verifies the machine's Ed25519 signature over a transcript
   bound to the ticket, channel, nonces and both ephemeral keys. A relay cannot impersonate the
   machine.
4. **Client authorisation.** A client needs an API ticket bound to its ephemeral key, machine and
   scopes. A stolen ticket alone cannot complete the handshake without the client private key.
5. **Least privilege.** Read scopes are the default; writes, MCP writes, SSH and exec are off until
   enabled on the machine and granted in the ticket. The machine enforces the intersection.
6. **Replay and tamper detection.** Nonces, single-use ticket `jti`, per-direction sequence numbers
   and AES-GCM AAD reject replay, reordering and header tampering.
7. **No plaintext logs.** The relay/API log ids and outcomes only. The machine's existing redaction
   rules still apply to session events, commands and supervisor logs.
8. **Supply-chain provenance.** npm and PyPI publishes use OIDC, provenance/attestations, pinned
   actions, checksums and a human-published draft release.
9. **Revocation.** A revoked machine or account fails closed within the ticket/credential cache
   windows; the owner can rotate the machine key.

### 8.3 Threat table

| # | Adversary / scenario | Capability | Mitigation | Residual risk |
|---|---|---|---|---|
| 1 | Passive network attacker | observes TLS/WSS | TLS 1.3; E2E AES-GCM; no tokens in URLs | traffic analysis of sizes/timing |
| 2 | Malicious or compromised relay | forwards, drops, reorders, sees metadata | E2E keys; machine signature; ticket cnf; sequence/AAD | DoS; metadata; relay can refuse service |
| 3 | Compromised API signing key | mints tickets/credentials | machine verifies ticket scopes and `mkey`; owner revocation; key rotation; audit | an attacker with a forged ticket **and** a client key can send allowed commands; API is the authorisation root |
| 4 | Stolen client device or ticket | has a live ticket | 300 s TTL; ephemeral key in memory; cnf binding; account session revoke; scope clamp | an unlocked device with a live session can act until revoked |
| 5 | Stolen machine or `relay.key` | has the machine key | revoke machine; rotate key; machine credential expires; no cloud copy | an attacker with the live key can impersonate the machine to clients until revoked |
| 6 | Malicious MCP client / stolen OAuth token | calls hosted MCP | OAuth 2.1 PKCE, audience, scopes; machine policy; read-only default; no write without both | a stolen token can read allowed data until expiry |
| 7 | Replay / MITM | replays frames or handshakes | nonces, `jti`, transcript hash, sequence, AAD, `mkey` signature | none for a correct implementation; crypto bugs are the risk |
| 8 | DoS / resource exhaustion | floods connections or frames | Cloudflare WAF/rate limits, one machine connection, 32 clients, 64/128 channels, 1 MiB frames, backpressure, idle timeouts | a determined attacker can still consume quota; no SLA in v1 |
| 9 | DNS/BGP/CA compromise | redirects `relay`/`api` | TLS system roots; `mkey` pin in ticket; client machine pin; HSTS; DNSSEC where available | a CA that mis-issues for both the API and relay could enable a targeted attack; machine key pin limits it |
| 10 | Supply-chain package compromise | malicious npm/PyPI package | no install scripts; OIDC; provenance; checksums; platform packages are data-only; pinned actions | a compromised build runner or owner account can still publish |
| 11 | Cloudflare account compromise | changes Workers/DNS | account MFA, least-privilege tokens, audit, E2E keys | attacker can deploy code that drops service but cannot decrypt past payloads |
| 12 | Contabo provider/console compromise | reads user-data, disk | short-lived enrollment token; no cloud private key; encrypted backup; host firewall | provider can read the running machine's memory; use a dedicated host |
| 13 | Cross-tenant access | another account's ticket | ticket `acct`/`machine` binding; relay and machine check; D1 scoping | a bug in a shared Worker could cross tenants; isolated DO per machine limits it |
| 14 | Sensitive data in logs | prompts/tool output in logs | payloads never logged; existing redaction; audit ids only | a machine-side application log may still contain data; operator review |
| 15 | SSRF/path traversal in HTTP channel | client sends arbitrary paths | `/api/v1/` prefix, normalisation, method/header/query allowlists, no static routes | a bug in a normaliser; tests cover encoded separators |
| 16 | SSH host-key substitution | MITM on the SSH layer | OpenSSH host-key verification through the E2E channel; `HostKeyAlias`; known_hosts | the owner must verify the first host key; relay cannot see it |
| 17 | Malicious machine config | enables broad exec/ssh | config is owner-owned 0600; scopes still need tickets; exec allowlist | the machine owner is the trust root |
| 18 | XSS in the PWA | steals session or keys | CSP, no token in localStorage, HttpOnly cookie, ephemeral keys, no `innerHTML` for data | a browser compromise or a dependency XSS remains |

### 8.4 What the relay/API can and cannot do

**The relay can:** see machine and account ids, client kind, ticket jti, public keys, channel ids,
frame sizes and timing; drop, delay, reorder or close connections; observe IP addresses; refuse
service.

**The relay cannot:** read or forge E2E payloads; sign a machine challenge; complete a client
handshake without the client ephemeral private key; mint a ticket; change a machine's scopes; read
a prompt, tool result, file or SSH byte.

**The API can:** authenticate accounts, register machines, mint tickets and credentials, revoke
machines, and read audit metadata. **The API cannot:** decrypt an E2E channel; sign as a machine;
read a machine's private key; read prompts or tool output. A compromised API can authorise a
malicious client with a key it controls; that is the authorisation-root residual risk in row 3.

### 8.5 Non-goals

- Multi-user accounts inside one machine, roles, quotas or billing.
- Anonymity or hiding machine/account metadata from Cloudflare.
- Protection from a compromised machine, a malicious root user, or a compromised browser.
- A sandbox for arbitrary remote commands; `ssh`/`exec` are owner-enabled trust, not isolation.
- Windows support in v1.
- A general-purpose VPN or TCP proxy; `tcp` is limited to the configured SSH target.
- Multi-project relay routing in v1; one selected project per machine.
- Hosted MCP write tools beyond the v1 list; job creation, capacity, tunnels, secret requests and
  session handoff remain local.
- E2E for control metadata; tickets and public keys are visible to the relay by design.

## 9. Tests and gates

### 9.1 Hermetic rules

Every slice follows the repository rules:

- Temporary homes and temporary repositories only; never the real `$HOME`, `~/.vowp` or
  `~/.stackspirit`.
- A private tmux socket with `-L <unique>` **and** a private `TMUX_TMPDIR`; create the directory
  first, refuse `default`, unset `TMUX`/`TMUX_PANE`.
- No network. A fake relay, fake API, fake `ssh`, fake `systemctl`, fake `ufw`, fake `rclone` and
  fake `age` are used where the behaviour is not the subject.
- Fake `stackspirit`/`vowp` binaries for launcher tests; no real harness.
- No secret values in fixtures, logs, generated units or test output. Test keys are clearly
  test-only and are never used by production code.
- Cloudflare tests use Miniflare/vitest with a local D1 and a local JWKS; no Cloudflare account.
- npm/PyPI tests install from local tarballs/wheels with `--offline`/`--no-index`.

### 9.2 Cross-language vectors

The E2E implementation has one committed fixture,
`crates/vowp-agent/tests/fixtures/relay-vectors.json`, with `test_only: true`. It contains a fixed
P-256 client key, P-256 machine key, nonces, ticket jti, transcript hash, `c2m`/`m2c` keys,
plaintext, nonce, AAD and ciphertext. The Rust test (`crates/vowp-agent/tests/relay_vectors.rs`)
and the TypeScript test read the same file and must produce byte-identical results. A change to the
transcript, HKDF labels, nonce layout or AAD fails both tests. The fixture uses only test keys and
is excluded from the published packages. The crypto test lives in `vowp-agent`, not
`vowp-protocol`, so the pure-data crate does not acquire AES/P-256 dependencies.

### 9.3 Test matrix

| Behaviour | Test | What must fail if absent |
|---|---|---|
| Machine auth | `relay_rejects_a_bad_machine_signature`, `relay_rejects_a_replayed_challenge_nonce` | a relay that accepts any signature or a replayed nonce |
| Ticket binding | `relay_channel_rejects_a_scope_escalation`, `relay_channel_rejects_a_replayed_ticket`, `relay_rejects_an_expired_ticket` | a client with a stolen or broadened ticket |
| E2E crypto | `relay_crypto_matches_the_cross_language_vector`, `relay_crypto_rejects_tampered_ciphertext` | a relay-readable or malleable channel |
| Frame caps | `relay_frame_cap_closes_without_partial_frame` | a buffered oversized frame |
| Machine connector | `machine_connector_reconnects_with_backoff`, `machine_connector_refreshes_a_credential` | a connector that gives up or uses an expired credential |
| Enrollment | `relay_enroll_writes_private_files_0600`, `relay_enroll_does_not_log_the_token` | a world-readable key or a token in a log |
| Relay routing | `relay_worker_forwards_only_the_bound_channel`, `relay_worker_supersedes_a_second_machine` | a client reaching another machine or two live machines |
| Relay blindness | `relay_worker_never_decrypts_data` | a relay that logs or parses plaintext |
| API | `api_enrolls_a_machine_once`, `api_rejects_an_expired_token`, `api_ticket_scopes_are_clamped` | token reuse or scope escalation |
| Hosted MCP | `hosted_mcp_forwards_tools_list_and_call`, `hosted_mcp_rejects_write_without_scope` | a write reachable read-only |
| Web | `web_client_decrypts_the_vector_channel` | a browser client that cannot complete E2E |
| SSH | `ssh_proxy_carries_openssh_bytes`, `ssh_proxy_requires_the_ssh_scope` | a plaintext or unauthorised SSH path |
| Cloud-init | `cloud_init_blocks_smtp_egress`, `cloud_init_enrolls_once_and_deletes_the_token`, `cloud_init_enables_no_backup_by_default` | a machine that can send mail, re-enrolls or backs up without consent |
| Backup | `backup_script_encrypts_before_upload`, `backup_script_excludes_private_keys`, `backup_script_retains_thirty_days` | a plaintext or key-bearing backup |
| Launchers | the npm and PyPI tests in §3.6 and §4.6 | a broken `npx`/`uvx` promise |
| Release | the workflow tests in §5.7 | a stored registry token or a non-OIDC publish |

### 9.4 Every slice's repository gates

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

The docs slices add `bash docs/tests/run.sh` and `python3 docs/reference/check-reference.py`. The
packaging slices add the npm/PyPI gates; the Cloudflare slice adds `pnpm --filter ./cloudflare/...
test` and the cross-language vectors. Every report states the exact commands and results.

## 10. Slice breakdown

Seven slices. Slice 1 is the layout prerequisite; slices 2 and 3 can run in parallel after it;
slice 4 follows 2 and 3; slice 5 follows 1 but must not run concurrently with slice 2 because both
touch `crates/vowp-cli/src/cli.rs`; slice 6 follows 5; slice 7 follows 4 and 6. The supervisor's
existing `owns` overlap rule serializes any pair that shares a path, including `cli.rs` and
`docs/guides/relay.md`. Each slice is one job, commits on its own branch, never pushes and never
publishes.

| Slice | `needs_done` | One-line result |
|---|---|---|
| `v6-9-public-monorepo` | none | one public MIT repo with both histories, CI and licence gates |
| `v6-10-npm-launcher` | `v6-9` | public `stackspirit` npm root plus four platform packages; `npx` works |
| `v6-11-pypi-wheels` | `v6-9` | four `stackspirit` wheels; `uvx` works |
| `v6-12-oidc-release` | `v6-9`, `v6-10`, `v6-11` | draft release then OIDC npm/PyPI publish and install smoke tests |
| `v6-13-relay-protocol-and-machine` | `v6-9` | relay types, E2E crypto, machine connector and `relay enroll/status/run` |
| `v6-14-relay-service-and-clients` | `v6-13` | Cloudflare relay/API/MCP, PWA client and `stackspirit ssh` |
| `v6-15-contabo-enrollment` | `v6-12`, `v6-14` | cloud-init, SMTP block, systemd units and optional R2 backup |

A later slice depends on an earlier slice's public types and endpoints; it does not change an
earlier slice's files except where the shared CLI registration or docs index is named explicitly.
The supervisor must serialize slices 5 → 6 → 7 because `crates/vowp-cli/src/cli.rs` and the relay
docs are touched by more than one of them.

### 10.1 `v6-9-public-monorepo`

**Goal.** Import the UI history under `apps/`, keep the engine at the root, make the repository
public MIT with a licence/provenance audit, and make CI path-aware. No behaviour change in the
engine.

**Owns.**

```text
.github/CODEOWNERS
.github/workflows/ci.yml
.github/workflows/security.yml
.github/pull_request_template.md
apps/**
Cargo.toml                                (repository/homepage metadata only)
LICENSE
NOTICE
README.md
CONTRIBUTING.md
SECURITY.md
CODE_OF_CONDUCT.md
SUPPORT.md
pnpm-workspace.yaml
scripts/monorepo/import-history.sh       (new)
scripts/monorepo/check-layout.sh         (new)
scripts/monorepo/check-versions.sh       (new)
scripts/monorepo/check-licenses.sh       (new)
scripts/monorepo/test-import.sh          (new)
scripts/audit/**                          (public-tree manifest and checks for the new layout)
docs/design/README.md
docs/design/relay-and-packaging.md       (status line only)
packaging/npm-stackspirit/**             (moved to packaging/legacy/npm-vowp)
packaging/pypi-stackspirit/**            (moved to packaging/legacy/pypi-stackspirit)
packaging/legacy/npm-vowp/**             (moved, frozen)
packaging/legacy/pypi-stackspirit/**     (moved, frozen)
```

**What changes.**

- Import the UI tree and its tags as in §2.2; the old repositories are untouched.
- Add the root licence, notice, contribution and security files; keep the engine docs under
  `docs/`.
- Add the version, layout and licence checks; make `check-versions.sh` fail on any mismatch.
- Split CI into path-filtered workflows with a required aggregate; add `CODEOWNERS`.
- Move the legacy public `vowp` npm and private PyPI templates to `packaging/legacy/` and mark
  them frozen. The legacy npm package is not published from this repository.
- Update the design index to mention this page; no other behaviour.

**Acceptance.**

- `scripts/monorepo/check-layout.sh` finds the engine workspace at the root and the UI at
  `apps/desktop` and `apps/web`.
- `scripts/monorepo/test-import.sh` proves both histories and tags survive on two tiny temp repos.
- `scripts/monorepo/check-licenses.sh` finds MIT on every first-party package and no incompatible
  UI file.
- `scripts/monorepo/check-versions.sh` rejects a mutated npm/PyPI/Homebrew version.
- `git log --follow` reaches a pre-import commit for one engine file and one UI file.
- The full-history secret scan is clean or has an owner-approved, rotated exception; no secret
  value is printed.

**What must fail if absent.** The layout test fails if the UI is still a sibling repository or the
engine moved under `engine/`; the import test fails if `--to-subdirectory-filter apps` is missing or
tags collide; the licence test fails on a missing MIT manifest; the version test fails on a drift;
the secret scan fails on a committed token.

**Gates.**

```sh
bash scripts/monorepo/check-layout.sh
bash scripts/monorepo/test-import.sh
bash scripts/monorepo/check-licenses.sh
bash scripts/monorepo/check-versions.sh
bash docs/tests/run.sh
bash scripts/tests/run.sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

### 10.2 `v6-10-npm-launcher`

**Goal.** Replace the private postinstall downloader with a public thin launcher and four platform
packages; prove `npx -y stackspirit` and `npx -y stackspirit mcp`.

**Owns.**

```text
packaging/npm/**
scripts/release/build-npm-packages.sh      (new)
scripts/tests/npm.sh                       (new)
crates/vowp-cli/src/mcp/mod.rs             (no-flag read-only mode)
crates/vowp-cli/src/mcp/backend.rs
crates/vowp-cli/src/mcp/tools.rs
crates/vowp-cli/src/cmd/mcp.rs
crates/vowp-cli/src/cli.rs
crates/vowp-cli/tests/mcp.rs
```

**What changes.**

- Add the root package, four platform templates, `lib/launch.js`, the four `bin/*.js` shims, tests
  and README. The package README documents the npm path; the shared install guide is updated once
  in `v6-15` after all launchers exist.
- Remove `private`, the `postinstall` downloader, `STACKSPIRIT_RELEASE_BASE_URL` and the vendored
  archive logic from the new package. The legacy package stays frozen under `packaging/legacy/`.
- Add the build script that turns release archives into platform tarballs and the root tarball.
- Add the no-flag read-only `stackspirit mcp` mode from §3.7; keep the existing session mode and the
  15 tool names unchanged.
- Document the supported matrix, the musl/Windows message and the `vowp` migration.
- Do not publish; `private` is false only because the package is meant to be published by the
  owner-gated release job.

**Acceptance.**

- `npm pack` produces five tarballs with only runtime files and mode `0755` binaries.
- A temp install runs `stackspirit --version`, `stackspirit mcp` with an argument and the three
  compatibility names through fake binaries.
- A real `stackspirit mcp` child with no flags answers `initialize`, lists exactly the five read
  tools and refuses a write with JSON-RPC `-32601`.
- `--omit=optional` produces the reinstall message; a version mismatch is refused.
- No package manifest has an install hook; no test needs the network.

**What must fail if absent.** The launcher test fails if `require.resolve` cannot find the platform
package; the pack test fails if a postinstall or a test file is present; the MCP test fails if the
argument is dropped or the no-flag server exits with a missing-flag error; the write test fails if a
read-only server opens a write path; the version test fails if a mixed install runs.

**Gates.**

```sh
node --test packaging/npm/test/*.test.js
bash scripts/tests/npm.sh
npm pack --dry-run --json packaging/npm/stackspirit/package.json
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

### 10.3 `v6-11-pypi-wheels`

**Goal.** Publish four self-contained `stackspirit` platform wheels and prove `uvx stackspirit`.

**Owns.**

```text
packaging/pypi/**
scripts/release/build-pypi-wheels.sh       (new)
scripts/tests/pypi.sh                      (new)
```

**What changes.**

- Add the PEP 517 backend, `pyproject.toml`, launcher, tests and README. The package README
  documents the PyPI path; the shared install guide is updated once in `v6-15`.
- Add the four wheel tags and the version mapping; refuse an sdist and a non-final publish version.
- Add the build script that maps release targets to wheel tags.
- Remove the `Private :: Do Not Upload` classifier from the new package; the old private template
  stays frozen under `packaging/legacy/`.

**Acceptance.**

- Four wheels build with fake binaries; each has the exact member list and tag.
- `pip install --no-index` in a temp venv runs all four console scripts.
- `uv tool run --from <wheel> stackspirit --version` runs the fake binary; CI fails if `uv` is
  missing.
- Two builds are byte-identical; `RECORD` hashes verify.
- An unsupported platform has no wheel, and the README says so.

**What must fail if absent.** The tag test fails if `WHEEL` and the filename disagree; the install
test fails if a binary is missing; the uvx test fails if the wheel is not self-contained; the
repro test fails if timestamps or ordering drift.

**Gates.**

```sh
python3 -m unittest discover -s packaging/pypi/tests
bash scripts/tests/pypi.sh
python3 -m build --wheel --no-isolation packaging/pypi/stackspirit
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

### 10.4 `v6-12-oidc-release`

**Goal.** Build every artifact on a tag, create a draft GitHub release, and publish npm/PyPI only
from a human-published release using OIDC; prove the install promises with clean-runner smoke tests.

**Owns.**

```text
.github/workflows/release.yml
scripts/release/package-archive.sh
scripts/release/check-packages.sh
scripts/release/publish-crates.sh
scripts/release/version-test.rs
scripts/release/build.rs
scripts/release/check-release.sh           (new)
scripts/release/test-workflow.sh           (new)
scripts/install.sh
scripts/uninstall.sh
scripts/vowp-watchdog.sh
scripts/vowp-watchdog.cron
packaging/homebrew/**
docs/release/**
CHANGELOG.md
```

**What changes.**

- Replace the release workflow with the tag → draft → owner-publish → OIDC npm/PyPI sequence in §5.
- Build the four archives, four platform npm tarballs, root npm tarball and four wheels in the
  matrix; checksum and attest them.
- Add the install smoke tests `npx -y stackspirit@<version> --version`,
  `npx -y stackspirit@<version> mcp` with an initialize request, and
  `uvx stackspirit@<version> --version`.
- Keep the public VOWP `vowp-<version>-<target>.tar.gz` path for the public release; the private
  `stackspirit-<version>-<target>.tar.gz` path stays for the StackSpirit artifacts.
- Update the installer/uninstaller for the monorepo paths and both product names; no publishing is
  run by the slice.
- Pin every action to a SHA; restrict permissions; add the `release`/`npm`/`pypi` environments.

**Acceptance.**

- `scripts/release/test-workflow.sh` proves OIDC, environments, no stored registry token, pinned
  actions, the four-target matrix and the draft gate.
- `scripts/release/check-packages.sh` still passes; `package-archive.sh` still refuses an archive
  missing a binary.
- A dry `workflow_dispatch` build produces all artifacts and publishes nothing.
- `scripts/install.sh` on a temp HOME installs both names and both daemons; a legacy fake home is
  updated in place, not duplicated.

**What must fail if absent.** The workflow test fails on `NPM_TOKEN`/`PYPI_API_TOKEN`, a missing
`id-token: write`, a publish job without an environment or a tag that publishes directly; the
archive test fails if a daemon is missing; the smoke test fails if only one name is checked; the
installer test fails if the legacy unit is duplicated.

**Gates.**

```sh
bash scripts/release/test-workflow.sh
bash scripts/release/check-packages.sh
bash scripts/tests/run.sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

### 10.5 `v6-13-relay-protocol-and-machine`

**Goal.** Define the relay/remote wire types, the E2E crypto, the machine connector and the
machine-side CLI. A fake relay and a fake client prove the machine control path and one HTTP read
without Cloudflare; MCP and SSH are wired by slice 6.

**Owns.**

```text
crates/vowp-protocol/src/relay.rs          (new)
crates/vowp-protocol/src/remote.rs         (new)
crates/vowp-protocol/src/lib.rs
crates/vowp-protocol/src/schema.rs
crates/vowp-protocol/src/config.rs
crates/vowp-protocol/tests/relay.rs        (new)
crates/vowp-protocol/tests/schema.rs
crates/vowp-protocol/tests/config.rs
crates/vowp-agent/src/relay/mod.rs         (new)
crates/vowp-agent/src/relay/auth.rs        (new)
crates/vowp-agent/src/relay/crypto.rs      (new)
crates/vowp-agent/src/relay/connector.rs   (new)
crates/vowp-agent/src/relay/key.rs         (new)
crates/vowp-agent/src/lib.rs
crates/vowp-agent/Cargo.toml
crates/vowp-agent/tests/relay.rs           (new)
crates/vowp-agent/tests/relay_vectors.rs   (new)
crates/vowp-agent/tests/fixtures/relay-vectors.json (new, test-only)
crates/vowp-cli/src/cmd/relay.rs           (new)
crates/vowp-cli/src/cmd/mod.rs
crates/vowp-cli/src/cli.rs
crates/vowp-cli/src/setup/commands.rs
crates/vowp-cli/tests/relay.rs             (new)
docs/design/crates/vowp-protocol.md
docs/design/crates/vowp-agent.md
docs/design/crates/vowp-cli.md
docs/design/types.md
docs/reference/schema/**
```

**What changes.**

- Add `RelayFrame`, `RemoteFrame`, `RelayDataFrame`, `Scope`, `ClientKind`, `MachineState`,
  `MachineCapabilities`, `Jwk`, the close/error codes and the `[relay]` config table. The protocol
  types are pure data; no I/O.
- Add the P-256/HKDF/AES-GCM channel implementation and the canonical transcript; add
  `cargo add -p vowp-agent p256 aes-gcm hkdf base64`.
- Add `RelayKey` (a separate `relay.key`, not `node.key`), machine enrollment/credential/status
  files, the connector loop, heartbeat, reconnect and credential refresh.
- Add `stackspirit relay enroll|status|run|rotate-key`; `run` discovers the loopback daemon through
  `<home>/web-window` and never starts a second daemon.
- Add the `[relay]` table to `NodeFile`; update the one full `NodeFile` literal in
  `setup/commands.rs`.
- Add the protocol/config docs and regenerate the schema pages.
- No Cloudflare code and no client code in this slice; the fake relay in tests implements the
  control frames and forwards opaque data.

**Acceptance.**

- Every control frame and remote frame round-trips; unknown fields are ignored, unknown types are
  rejected, and every optional field defaults.
- The machine auth canonical string and signature match a fixed vector; a bad signature, replayed
  nonce or expired credential fails.
- The E2E vector matches the committed fixture byte-for-byte; a tampered ciphertext, header, nonce
  or sequence is rejected.
- A fake relay + fake client runs one HTTP read through the connector; an MCP or TCP frame
  returns a typed `not_implemented` until slice 6 wires it. The connector never writes a payload
  to its log.
- Enrollment writes `relay.key` 0600, `relay.pub` 0644 and `relay.json` 0600; the token never
  appears in a log or status file.
- A dropped relay connection reconnects with backoff; a revoked machine stops.
- `stackspirit relay status --json` has no credential, token or private key.

**What must fail if absent.** The crypto test fails if the relay can decrypt or the vector differs;
the auth test fails if any signature is accepted; the connector test fails if a frame is not
forwarded or is logged; the enrollment test fails on a group/world-readable file; the scope test
fails if an unenabled channel is accepted.

**Gates.**

```sh
cargo test -p vowp-protocol --test relay
cargo test -p vowp-protocol --test config
cargo test -p vowp-agent --test relay
cargo test -p vowp-agent --test relay_vectors
cargo test -p vowp-cli --test relay
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
python3 docs/reference/check-reference.py
```

### 10.6 `v6-14-relay-service-and-clients`

**Goal.** Deploy the Cloudflare relay/API, the hosted MCP and the PWA client, and add
`stackspirit ssh` over the relay. The Rust machine connector from slice 5 is the server side.

**Owns.**

```text
cloudflare/relay/**
cloudflare/api/**
cloudflare/mcp/**
apps/web/**
apps/desktop/**                            (relay client integration only)
crates/vowp-agent/src/relay/client.rs      (new; client-side transport)
crates/vowp-cli/src/cmd/ssh.rs             (new)
crates/vowp-cli/src/cmd/relay_proxy.rs     (new)
crates/vowp-cli/src/cmd/mcp_remote.rs      (new)
crates/vowp-cli/src/cmd/mod.rs
crates/vowp-cli/src/cli.rs
crates/vowp-cli/src/lib.rs
crates/vowp-cli/tests/ssh.rs               (new)
crates/vowp-cli/tests/relay_proxy.rs       (new)
docs/design/cloud-relay.md                 (new)
docs/guides/relay.md                       (new)
```

**What changes.**

- Add the relay Worker and `MachineHub` Durable Object: WebSocket upgrade, machine/client auth,
  ticket validation, channel routing, presence, limits, close codes and no payload logging.
- Add the API Worker and D1 migrations: device flow, account sessions, enrollment tokens, machine
  records, credentials, key rotation, relay tickets, JWKS, hosted MCP OAuth metadata, audit and
  internal presence.
- Add the hosted MCP Worker: OAuth 2.1 resource server, `initialize`, `tools/list`, `tools/call`,
  the v1 tool mapping, scope filtering and channel pooling.
- Add the PWA remote client: WebCrypto P-256/AES-GCM, ticket request, channel, HTTP/SSE frames and
  the existing board/session/inbox/cost views.
- Add `stackspirit ssh`, `relay proxy` and `mcp --remote`; the ProxyCommand path uses OpenSSH and
  the `tcp` channel to `127.0.0.1:22`.
- Add the Cloudflare local test setup (Miniflare, D1 fixture, local JWKS) and the cross-language
  vectors shared with slice 5.
- No Cloudflare secrets, account ids or production tokens are committed; `wrangler.toml` uses
  environment placeholders and `wrangler deploy` is an owner operation.

**Acceptance.**

- A fake machine + fake client traverses the relay: machine auth, ticket validation, channel open,
  E2E handshake, one HTTP read and one MCP read; the relay's logs contain no payload.
- A client with another account's ticket or a broadened scope is refused; a replayed ticket is
  refused.
- A second machine connection supersedes the first; a machine disconnect closes its clients with
  `machine_offline`.
- The API enrolls once, rejects an expired token, clamps scopes and rotates JWKS without breaking
  a live key.
- Hosted MCP `tools/list` includes only the v1 read tools (and write tools only with `mcp.write`);
  a write without scope is a JSON-RPC error.
- The PWA test completes the vector channel in Node's WebCrypto and renders one board frame.
- `stackspirit ssh` with a fake local `ssh` and a fake machine `sshd` carries bytes both ways; a
  missing `ssh` scope is refused before a `tcp.open`.
- The cross-language fixture matches in Rust and TypeScript.

**What must fail if absent.** The relay test fails if a payload is logged or a cross-account
channel is forwarded; the API test fails on token reuse; the MCP test fails if a write is reachable
read-only; the PWA test fails if E2E cannot decrypt; the SSH test fails if the proxy opens a TCP
stream without the scope.

**Gates.**

```sh
pnpm --filter ./cloudflare/relay test
pnpm --filter ./cloudflare/api test
pnpm --filter ./cloudflare/mcp test
pnpm --filter ./apps/web test
cargo test -p vowp-cli --test ssh
cargo test -p vowp-cli --test relay_proxy
cargo test -p vowp-agent --test relay
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

### 10.7 `v6-15-contabo-enrollment`

**Goal.** Make a Contabo VPS enroll itself with one cloud-init template: install StackSpirit, block
SMTP egress, enroll with a one-time token, start the services, and leave backups off by default.
Add the optional encrypted R2 backup hook and the Iroh transport-hint documentation.

**Owns.**

```text
deploy/cloud-init/**
deploy/backup/**
deploy/systemd/**
scripts/tests/cloud-init.sh                (new)
scripts/tests/backup.sh                    (new)
docs/guides/contabo.md                     (new)
docs/guides/relay.md                       (update)
docs/security.md                           (relay, cloud-init and backup sections)
docs/guides/install.md                     (npm, PyPI, relay and VPS sections)
```

**What changes.**

- Add the Contabo `#cloud-config` template with a non-root user, UFW/nftables SMTP block, verified
  installer, one-time enrollment, token deletion and the two systemd units.
- Add a guarded, idempotent enrollment path: an existing `relay.json` is not re-enrolled; a missing
  or expired token stops with instructions instead of a retry loop.
- Add the optional R2 backup script and timer, disabled by default; the age identity stays offline;
  the script excludes private keys and secrets.
- Add the operator guide, the security page sections, the Iroh transport-hint note and the single
  shared install guide (npm, PyPI, relay and VPS paths) after all launchers exist.
- No real token, bucket name, account id or key is committed; placeholders are obvious.

**Acceptance.**

- `cloud-init` schema validation passes; `shellcheck` passes; the fake `ufw`/`nft` log shows the
  three SMTP deny rules and no inbound port except 22.
- A fake enrollment run with a valid token creates `relay.json`, deletes the token file and enables
  both units; a second run is a no-op; an expired token leaves a clear error and no retry loop.
- The default run installs no backup timer and no R2 config.
- The backup test proves encryption happens before upload, the key/secret/relay files are excluded,
  the retention deletes only old objects and the timer is disabled.
- Every command in the guide exists or is marked planned; docs links and command checks pass.

**What must fail if absent.** The cloud-init test fails if an SMTP port is reachable in the fake
firewall, if the token remains on disk, or if a second enrollment occurs; the backup test fails if
a private key is in the tar or an object is uploaded unencrypted; the docs test fails on a missing
page or a broken link.

**Gates.**

```sh
bash scripts/tests/cloud-init.sh
bash scripts/tests/backup.sh
bash docs/tests/run.sh
python3 docs/reference/check-reference.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## 11. Risks and open questions

### 11.1 Stage 1 risks

| Risk | Mitigation |
|---|---|
| A secret or non-MIT file in either history leaks at publication | full-history scan, licence audit, owner-approved rewrite and rotation before the repository becomes public |
| The UI toolchain cannot sit under `apps/` without a root lockfile conflict | keep the UI's own lockfile and build directory; the engine CI never installs UI dependencies |
| `@stackspirit` scope or the PyPI `stackspirit` name is unavailable | owner reserves the names before the first publish; fallback platform names are `stackspirit-<platform>` and the PyPI CLI fallback is an owner decision |
| A platform package is missing or mismatched | the launcher reports the exact package and version; the release smoke test installs every supported platform in CI where possible |
| Alpine/musl, Windows or older glibc users cannot install | documented unsupported matrix; a musl package and a manylinux_2_28 build are additive later |
| macOS Gatekeeper blocks ad-hoc-signed binaries | Developer ID signing/notarisation as an owner action; the README states the requirement |
| npm/PyPI trusted publishing is misconfigured | OIDC fails closed with the exact publisher setup message; no stored-token fallback |
| A bad version is published | registries are immutable; publish a patch and deprecate the bad version; the draft gate catches most errors before publish |
| The release workflow is too broad | publish jobs download prebuilt artifacts and only publish; build jobs have no registry credentials |
| The public VOWP release is accidentally changed | the monorepo workflows never target `syver777/vowp`; a separate release line remains |

### 11.2 Stage 2 risks

| Risk | Mitigation |
|---|---|
| A crypto bug makes the relay readable or a channel malleable | one cross-language fixture, tamper/replay tests, no custom primitives, review of the transcript and HKDF labels |
| Cloudflare Durable Object cost or limits | one DO per machine, hibernation API, 64/128 channels, 1 MiB frames, backpressure and quotas; a paid Workers plan is an owner prerequisite |
| Cloudflare outage or account compromise | local operation never depends on the relay; E2E protects past payloads; owner MFA and least-privilege tokens |
| API signing-key compromise authorises a malicious client | machine key pin in tickets, scope clamp, revocation, audit, rotation; API is the authorisation root and is documented as such |
| A stolen machine key | revoke/rotate; credential expiry; machine owner can see the audit; no cloud copy |
| Browser WebCrypto differences | P-256, HKDF and AES-GCM are the common WebCrypto set; a Node vector test runs in CI; an unsupported browser gets a clear error |
| Hosted MCP OAuth compatibility varies by client | dynamic client registration, PKCE, resource metadata; start with read tools; document the tested clients |
| v1 exposes one project per machine | the machine card names the project; multi-project routing is a later slice; `ssh` gives full CLI access |
| `stackspirit ssh` depends on OpenSSH and `sshd` | documented requirement; `--exec` is a later fallback; Windows is unsupported |
| Relay/API metadata is sensitive | no payload logs; retention limits; audit ids only; a privacy note in the guide |
| A malicious client sends SSRF/path traversal | explicit `/api/v1/` allowlist, normalisation, method/header/query rules, no static routes, fuzz tests |
| Cloud-init token is visible in user-data | 15-minute single-use token, file removal, key rotation if exposed; the console path is offered |
| R2 backup leaks or is unreadable | age encryption before upload, offline identity, exclude list, checksum verify, disabled by default, restore test |

### 11.3 Owner decisions

These are recommendations, not silent choices. The owner must accept or change them before the
corresponding slice is dispatched.

1. **Monorepo root.** Recommend the engine at the root and the UI under `apps/`, with the old
   repositories archived read-only for 90 days. Alternative: a new empty repository with both
   under `engine/` and `apps/`, which touches the root package and release paths.
2. **Public cutover.** Recommend a staging repository, full-history audit, rename to
   `Stack-Spirit/stackspirit`, archive the old UI and engine repositories, and keep
   `syver777/vowp` separate. Alternative: keep the engine private and publish only packages,
   which does not meet the public-monorepo goal.
3. **npm names.** Recommend `stackspirit` plus `@stackspirit/linux-x64`, `linux-arm64`,
   `darwin-x64`, `darwin-arm64`. Alternative: unscoped `stackspirit-<platform>` if the scope is
   unavailable.
4. **PyPI floor.** Recommend `manylinux_2_35` / macOS 11 and no sdist. Alternative: a
   `manylinux_2_28` build container for older Linux, which is additive.
5. **Publish gate.** Recommend tag → draft GitHub release → owner publishes → OIDC npm/PyPI, with
   protected environments. Alternative: publish directly on tag, which removes the human review.
6. **E2E mandatory.** Recommend the P-256/AES-GCM channel in v1, with the relay seeing only
   metadata. Alternative: TLS-only relay trust, which is simpler but lets the relay read prompts
   and tool output; this design does not recommend it.
7. **`stackspirit ssh`.** Recommend OpenSSH over the E2E `tcp` channel to loopback `sshd`, with no
   cloud SSH keys. Alternative: a first-party PTY shell (`exec`), which is a larger security surface
   and is defined but disabled.
8. **Remote scopes.** Recommend read-only HTTP/MCP by default; writes, SSH and exec off until the
   owner enables them. Alternative: writes on by default for convenience; not recommended.
9. **Relay project scope.** Recommend one selected project per machine in v1, with `ssh` for the
   rest. Alternative: a multi-project connector, which needs a home-wide daemon or per-project
   fan-out.
10. **Hosted MCP tools.** Recommend the eight v1 tools in §6.6 and read-only defaults; the other
    seven tools remain local until a machine-side MCP channel and a lease model exist.
11. **Contabo SSH.** Recommend keeping public `22/tcp` only for bootstrap and removing it after
    `stackspirit ssh` works; `sshd` may listen on loopback. Alternative: close 22 immediately and
    use the provider console, which is safer but less convenient.
12. **R2 backup.** Recommend opt-in, age-encrypted, offline identity, 30-day retention and no timer
    by default. Alternative: provider snapshots only, which the operator guide states are outside
    StackSpirit.
13. **Iroh.** Recommend protocol hints now and no Iroh dependency in these slices. Alternative:
    implement Iroh in Stage 2, which adds a feature flag and a hole-punching test matrix.

### 11.4 Open questions

1. Which npm scope and PyPI publisher accounts exist today, and who owns the trusted-publisher
   configuration?
2. Which Cloudflare account/zone hosts `stackspirit.io`, and is a paid Workers plan available for
   Durable Objects?
3. Which account identity provider backs `api.stackspirit.io` (StackSpirit account, GitHub, Google,
   passkey-only)?
4. Does the UI tree contain any file that cannot be MIT-licensed, and who approves the public
   cutover?
5. Is a first public version `0.6.0`, or does the owner keep the existing `0.5.1-dev` line until the
   Stage 2 foundation is complete?
6. Does the owner want remote HTTP writes in the first relay release, or read-only plus `ssh`?
7. Does the owner want the public VOWP repository mirrored from the monorepo, archived, or kept as
   an independent release line?
8. Is `mcp.stackspirit.io` intended for third-party MCP clients (ChatGPT, Claude) at launch, or
   only for StackSpirit Desktop/CLI?

## 12. Status of this page

This is a **design proposal**. It defines the target layout, the launcher and release contracts, the
exact relay control/remote/E2E schemas, the threat model, the cloud-init flow and seven slices. It
does not claim that any slice is implemented, does not dispatch the slices, does not publish a
package or deploy a Worker, and does not promise a release, tag, version or date. The current tree
remains the source of truth for existing behaviour; the v6-6 packaging and v6-7 docs work is the
starting point, not evidence for the target described here.

## 13. See also

- The v6 rebrand and compatibility contract:
  [stackspirit-engine-rebrand.md](stackspirit-engine-rebrand.md).
- The operator migration path: [stackspirit-migration.md](../guides/stackspirit-migration.md).
- The current WSS trust boundary and hardening history:
  [hardening.md](v0.4/hardening.md).
- Install and agent-node guides:
  [install.md](../guides/install.md) and [agent-node.md](../guides/agent-node.md).
- The security model and the roadmap:
  [security.md](../security.md) and [roadmap.md](../roadmap.md).
