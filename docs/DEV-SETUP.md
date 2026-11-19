# Developer setup — Linux desktop build container

The desktop application is a Tauri 2 host (`apps/spirit-desktop/src-tauri`) plus a
React/TypeScript UI (`apps/spirit-desktop/src`). Compiling the host on Linux needs the
GTK 3, WebKitGTK 4.1, Soup 3, and JavaScriptCore GTK 4.1 development packages. Those
packages are not installed on every worker or developer machine, and installing them
with `sudo` is not always possible.

`scripts/desktop-build-container.sh` solves that with a pinned Docker image. It installs
nothing on the host, uses no `sudo`, and keeps every generated artifact outside the
repository.

> The container proves the **Linux host compile** and the desktop UI gates. It does
> **not** prove a signed or notarized macOS bundle, a Windows installer, a real
> OS-keychain round trip, or an AppImage/deb bundle. Those remain native and CI checks.

## Prerequisites

- Docker Engine (or Docker Desktop) with a reachable daemon.
- Your user can run Docker, for example as a member of the `docker` group
  (`docker info` succeeds without `sudo`).
- About 4 GB of free disk for the image plus 2–4 GB for the Rust, Cargo, and npm caches
  after the first full check. The caches grow with each toolchain and dependency update.
- Network access for the first run: the image build pulls the pinned base images and
  installs Debian packages, and `check` downloads crates, npm packages, and the pinned
  engine release.
- Node.js is not required on the host for this workflow; Node 22 runs inside the image.

The script exits with a clear error if Docker is missing or the daemon is unreachable.
It never installs packages, never runs `sudo`, and never deletes repository files.

## Quick start

From any directory, inside the repository checkout:

```sh
scripts/desktop-build-container.sh check
```

The first run builds the image if it is missing; later runs reuse it. The script bind-
mounts the worktree read-write at `/work`, runs as your UID/GID, sets `HOME=/tmp`, and
runs these gates in this order:

```sh
cargo fmt --all -- --check
node scripts/fetch-engine.mjs
npm ci
npm run typecheck
npm run lint
npm test
npm run build
cargo check -p spirit-desktop
cargo test -p spirit-desktop
```

`npm ci` and `npm run build` run before the Rust steps because the Tauri build script
reads the frontend output directory (`../dist`, produced by `npm run build`).
`node scripts/fetch-engine.mjs` runs first because `tauri-build` validates
`bundle.externalBin` while compiling the host; the fetch is checksum-verified, skips
when the existing binary and lock match, and writes only the gitignored sidecar.

A successful run ends with:

```text
==> desktop-build: all desktop gates passed
```

and exits `0`. Each step prints a header such as:

```text
==> desktop-build: cargo check -p spirit-desktop
```

so a failure identifies the exact command. A failing step exits non-zero with that
command's output; the script does not continue past it.

On a warm cache the run is dominated by the Rust host compile and takes a few minutes;
the first run is slower because it builds the image and downloads crates and npm
packages.

This covers the Linux host compile and the desktop UI gates from `docs/PLAN.md` §10.6.
`npm run test:e2e` (Playwright) is deliberately not included: the image does not
download browser binaries. Run browser e2e on a host or in CI that installs Playwright
browsers. `cargo clippy -p spirit-desktop --all-targets -- -D warnings` also passes in
this image (run it through `scripts/desktop-build-container.sh shell`); it is not part
of the default `check` command.

## Workspace Rust gates

A fresh job worktree does not contain `apps/spirit-desktop/src-tauri/binaries/`; the
directory is gitignored. The `spirit-desktop` build script (`tauri-build`) validates the
sidecar while compiling the host, so `cargo clippy --workspace` and
`cargo test --workspace` abort with "resource path ... doesn't exist" before they read any
Rust code. Run workspace Cargo through `scripts/cargo-gate.sh`: it resolves the repository
root from its own path, runs the checksum-verified `node scripts/fetch-engine.mjs` (a
no-op when the pinned binary is present), and then `exec`s Cargo with the arguments,
quoting, stdio and exit code unchanged.

```sh
scripts/cargo-gate.sh fmt --all -- --check
scripts/cargo-gate.sh clippy --workspace --all-targets -- -D warnings
scripts/cargo-gate.sh test -p spirit-core
```

`vowp.toml` defines two named gate sets built on that wrapper:

| Gate set | Gates |
|---|---|
| `rust-check` | `node scripts/fetch-engine.mjs`; `scripts/cargo-gate.sh fmt --all -- --check`; `scripts/cargo-gate.sh clippy --workspace --all-targets -- -D warnings` |
| `rust-core-check` | The three `rust-check` gates plus `scripts/cargo-gate.sh test -p spirit-core` |

Create a job with `vowp job add ... --gates rust-check` (or `--gates rust-core-check`) and
the sidecar is fetched before the first Rust gate, with no hand-written fetch step in the
brief.

Desktop host builds and the desktop UI gates belong to
`scripts/desktop-build-container.sh check` (above). Do not install GTK, WebKitGTK, Soup
or JavaScriptCore development packages on the host; the pinned container is the supported
path, and `cargo-gate.sh` only prepares the sidecar for Cargo.

## Commands

| Command | What it does |
|---|---|
| `scripts/desktop-build-container.sh check` | Default. Image build if needed, then the Rust and UI gates listed above. |
| `scripts/desktop-build-container.sh tauri-build` | Fetches the pinned engine sidecar if missing, then runs `npx tauri build --debug --no-bundle` in `apps/spirit-desktop`. Best-effort: records a real host compile plus frontend bundle, no AppImage/deb. |
| `scripts/desktop-build-container.sh shell` | Interactive bash in the same image with the worktree at `/work`, `HOME=/tmp`, and the same caches. |
| `scripts/desktop-build-container.sh build` | Builds or refreshes the image only (`stackspirit-desktop-build:local`). |
| `scripts/desktop-build-container.sh --help` | Usage summary. |

Environment overrides:

| Variable | Default | Meaning |
|---|---|---|
| `STACKSPIRIT_DESKTOP_BUILD_IMAGE` | `stackspirit-desktop-build:local` | Docker image tag. |
| `STACKSPIRIT_DESKTOP_RUST_TOOLCHAIN` | `1.90.0` | rustup toolchain selected with `RUSTUP_TOOLCHAIN`. |
| `XDG_CACHE_HOME` | `~/.cache` | Base directory for the host caches. |

## What is in the image

`docker/desktop-build.Dockerfile` pins:

- `rust:1.90-bookworm` — Rust 1.90.0 with `rustfmt` and `clippy`, plus `pkg-config`,
  `build-essential`, `curl`, `wget`, `file`, `git`, `ca-certificates`, and `libssl-dev`.
  The workspace `rust-version` is 1.85, but the pinned `Cargo.lock` (Tauri 2.12 and
  its `tauri-utils`/`muda`/`uuid` dependencies) declares `rust-version = 1.90`; a 1.85
  compiler stops with "rustc 1.85.1 is not supported by the following packages". The
  image pins 1.90.0, the minimum version the locked tree accepts.
- `node:22.23.5-bookworm-slim` — Node 22 LTS and npm copied from the exact official
  image tag. No NodeSource setup script runs at build time.
- `libgtk-3-dev`, `libwebkit2gtk-4.1-dev`, `libjavascriptcoregtk-4.1-dev`,
  `libsoup-3.0-dev`, `libayatana-appindicator3-dev`, `librsvg2-dev`, `libxdo-dev`, and
  `patchelf`.

The image installs tools only; it never copies the repository into the image. The
build context is filtered by `.dockerignore`, which excludes `target`, `node_modules`,
`.git`, `.vowp`, `dist`, Playwright output, and local engine binaries and lock files.

`RUSTUP_TOOLCHAIN` is set so the repository's `rust-toolchain.toml` channel (`stable`)
cannot make the container download a newer compiler. It is `1.90.0`, not a moving
`stable` channel: a container run never updates Rust. Bumping `Cargo.lock` may raise
the required minimum; the image tag and this pin must move together.

To refresh the image after editing the Dockerfile or bumping a base tag:

```sh
scripts/desktop-build-container.sh build
```

Version tags for official images are immutable. For a fully digest-pinned workflow,
replace the `FROM` tags with `image@sha256:<index-digest>` and record the digest next to
the tag in this document.

## Caches and generated files

The script keeps all build state outside the worktree:

```text
${XDG_CACHE_HOME:-$HOME/.cache}/stackspirit/desktop-build/
├── cargo/    # CARGO_HOME: crates.io registry and git checkouts
├── target/   # CARGO_TARGET_DIR: Rust build artifacts, including the tauri-build debug binary
└── npm/      # npm_config_cache
```

The first `check` downloads the crates and npm packages into those directories; later
runs reuse them. Deleting that directory is safe and only forces a slower rebuild.

Inside the worktree the script leaves only normal, gitignored build output owned by
your UID: `node_modules/` and `apps/spirit-desktop/dist/`. It verifies after each run
that no root-owned path exists under the repository.

`check` also fetches the pinned StackSpirit Engine sidecar when it is missing:

```sh
node scripts/fetch-engine.mjs
```

The binary is written to `apps/spirit-desktop/src-tauri/binaries/opencode-<triple>` with
the generated `engine-lock.json`; both are gitignored and must never be committed. The
fetch verifies the pinned SHA-256 from `engine/manifest.json` before extraction. If the
binary is already present and verified, the fetch is a no-op. To re-verify:

```sh
bash scripts/verify-engine-checksum.sh
```

The debug Tauri application binary is written to
`${XDG_CACHE_HOME:-$HOME/.cache}/stackspirit/desktop-build/target/debug/spirit-desktop`.
A successful `tauri-build` ends with:

```text
    Finished `dev` profile [unoptimized + debuginfo] target(s) in ...
       Built application at: /cache/target/debug/spirit-desktop

==> desktop-build: tauri-build finished
```

Because `tauri build --no-bundle` skips packaging, no AppImage or `.deb` is produced.

## Troubleshooting

**`docker is not installed` or `the Docker daemon is not reachable`**
Install Docker Engine or start the daemon, and make sure your user can run `docker info`
without `sudo`. The script cannot work around a missing daemon.

**`permission denied` writing `/work`**
Run the script as the repository owner. The container uses `--user "$(id -u):$(id -g)"`;
running the script through `sudo` would create root-owned files and is rejected.

**A check fails inside the container**
The failure is a real repository failure (formatting, types, lint, tests, or the Rust
host compile), not a host-package problem. Run the printed command on the host or use
`scripts/desktop-build-container.sh shell` to reproduce it with the same toolchain.

**Out of disk space**
The image is roughly 3.5 GB and the target cache can reach a few GB. Remove old images
or the cache directory:

```sh
rm -rf "${XDG_CACHE_HOME:-$HOME/.cache}/stackspirit/desktop-build"
```

That path is outside the repository; the script never deletes repository files.

**The image needs rebuilding**
`check` and `tauri-build` reuse an existing `stackspirit-desktop-build:local` image.
Run `scripts/desktop-build-container.sh build` after changing the Dockerfile.

## One-time-password onboarding (`spirit-core`)

`crates/spirit-core/src/ssh/password_install.rs` holds the core API for onboarding a
server with a password that is used once and then forgotten:

```rust
let outcome = spirit_core::ssh::install_public_key_with_password(
    runner,   // Arc<dyn CommandRunner>; SystemCommandRunner in the app
    &request, // target, confirmed HostKeyPin, SecretString password,
              // <private_key_path>.pub and disable_password_auth
    &options, // app-private known_hosts and timeouts
)
.await?;
```

The first hop is the only one that relaxes `BatchMode`. A generated `SSH_ASKPASS`
helper (with `SSH_ASKPASS_REQUIRE=force`) reads the password from a `0600` file inside
a `0700` temp directory; only the helper path is exported, and the directory is
removed on every exit path. Before the password is written anywhere, an `ssh-keyscan`
plus `ssh-keygen` scan must match the confirmed `HostKeyPin`, and only that confirmed
key is written to the app-private `known_hosts`. A fresh key-only `BatchMode=yes`
login runs `true`; the outcome cannot say `installed` until that hop succeeds.

Exercise it without a server through the fake-command harness:

```sh
cargo test -p spirit-core --test ssh_install
```

The tests cover the password staying out of argv, environment values and stdin; the
askpass file being `0600` inside a `0700` directory and gone after the call; the
`authorized_keys` merge being idempotent; a wrong password or a changed host key
returning a typed error with no temp files left; and a failed verify hop never
reporting `installed`.

A real-server run is **owner verification**. It needs a server you own with password
login enabled, a confirmed host-key fingerprint, and optionally passwordless sudo
(`sudo -n true`) plus `sshd -t`/`sshd -T`. The API never prompts for or accepts a sudo
password. When it applies the optional change, it writes the app-owned drop-in
`/etc/ssh/sshd_config.d/99-stackspirit-password-auth.conf`, validates it with
`sshd -t` and the effective `sshd -T` output, reloads sshd, and removes the drop-in
again if any step fails. If sudo or `sshd -t` is unavailable, that step returns a
typed `NotSupported` outcome and the key install itself stays successful. The desktop
card and Tauri command that call this API are a later job; nothing in the UI installs
a key yet.

## App-owned SSH keys (`spirit-core`)

`crates/spirit-core/src/ssh/keygen.rs` creates the key pair that the secure onboarding
flow installs. One call writes an app-private ed25519 pair and stores its passphrase in
the local OS keychain:

```rust
let key = spirit_core::ssh::create_app_key(
    keys_dir,     // app-private directory; created/chmodded 0700 on Unix
    "id_ed25519", // key name; also the file name and keychain account suffix
    &keychain,    // Keychain trait; OsKeychain in the app
    &spirit_core::ssh::AppKeyOptions::default(), // force: false
)?;

let unlocked = spirit_core::ssh::unlock_app_key(&key.private_key_path, &keychain)?;
```

The private key is written in OpenSSH format and encrypted with a generated 32-byte
random passphrase. The passphrase is stored only in the OS keychain under
`ssh-key:<name>`; it never enters argv, a log, a config file, or the returned structs.
`<dir>/<name>` and `<dir>/<name>.pub` are written `0600`, and `<dir>` is `0700` on
Unix. `create_app_key` returns the public-key line, the `SHA256:...` fingerprint, and
the key paths. It refuses to replace an existing pair unless `force` is set, in which
case both files and the keychain entry are replaced. `unlock_app_key` reads the stored
passphrase, decrypts the key, and returns the public line and fingerprint. A missing
keychain entry is a typed `keychain` error; a wrong passphrase is a typed `ssh_auth`
error, and neither message contains secret bytes.

The real keychain (`OsKeychain`) is behind the `keychain` feature. The module itself
compiles without it, and tests use a fake keychain. Exercise key creation without an
OS keychain or a server:

```sh
cargo test -p spirit-core --test ssh_keygen
```

The tests cover OpenSSH parsing and ed25519 key shape, public/private matching, file and
directory modes, one keychain write per create with no passphrase in the file or
`Debug` output, unlock success, missing/wrong passphrase typed errors, no-force refusal
with the first key untouched, force replacement, and stable/changing fingerprints. The
Tauri command, bridge methods and onboarding-card wiring are a later job; nothing in
the UI creates a key yet.

## What remains a native, macOS, or CI check

The Docker workflow is a Linux compile and test environment, not a release pipeline.
The following are **not** covered and remain owner or CI verification:

- a signed and notarized macOS `.app`/`.dmg`, and a Windows installer (Tauri bundles
  are native per platform);
- a real OS-keychain round trip. The container has no logged-in desktop session or
  platform credential store, so keychain integration is exercised by tests with a fake
  keychain; a real macOS/Windows/Linux Secret Service round trip must run on a real
  desktop session;
- AppImage and `.deb` packaging (`tauri build --no-bundle` skips bundling), Tauri
  update signing, and icon/installer verification;
- Playwright browser end-to-end flows (needs downloaded browsers plus a running
  preview server; not in the image);
- a visually inspected application window (the container has no display server);
- live SSH, VOWP, and local-orchestrator execution, which the product gates behind
  VOWP milestones and the `.asif.zip` gate.

The StackSpirit Engine sidecar is downloaded and checksum-verified but never committed.
It is fetched into a gitignored path for the host compile and `tauri-build`; see
`README.md` and `scripts/verify-engine-checksum.sh`.
