# Publishing VOWP to crates.io

This page is the maintainer procedure for the seven VOWP crates. It was written and exercised on
2026-11-09 with version `0.1.0`. **Nothing was published by the job that prepared it**: every command
below was either run as a dry run or is marked as a post-publish verification step.

The short version:

```sh
scripts/release/check-packages.sh          # package every crate and verify every dry run
scripts/release/publish-crates.sh          # same checks, then prints the planned publish order
CARGO_REGISTRY_TOKEN=... scripts/release/publish-crates.sh --execute
```

The last command is the only one that uploads. It reads the token from the environment, never from a
file, and never prints it. Run it from the final release commit after the pre-flight checklist passes.

## Crates and publish order

All crates use the workspace version (`[workspace.package] version` in the root `Cargo.toml`) and
`edition = "2024"`, `rust-version = "1.85"`, `license = "MIT"`. Inter-crate path dependencies also carry
`version = "0.1.0"`, which is what makes the packaged manifests usable outside this workspace.

| Order | Crate | What it is | Depends on |
|---:|---|---|---|
| 1 | `vowp-protocol` | Shared types, traits, configuration and schema export | - |
| 2 | `vowp-store` | SQLite schema, repositories, JSONL logs, blobs and reindex | `vowp-protocol` |
| 3 | `vowp-graph` | Job DAG, state machine, ownership checks and dispatch planning | `vowp-protocol` |
| 4 | `vowp-tmux` | Safe tmux window manager and its in-memory fake | - |
| 5 | `vowp-adapters` | Claude Code, opencode and generic CLI adapters | `vowp-protocol` |
| 6 | `vowp-core` | Sessions, gates, ledger, schedules, loops and promotion | protocol, store, graph, tmux, adapters |
| 7 | `vowp-cli` | The `vowp` binary and its command library (manifest at the workspace root; source in `crates/vowp-cli/src`) | all six above |

Publish strictly in this order. `vowp-store`, `vowp-graph`, `vowp-tmux` and `vowp-adapters` only need
`vowp-protocol`, so their relative order is interchangeable, but the script uses the order above so the
output is deterministic. `vowp-core` cannot resolve until all five of its dependencies are visible in the
index, and `vowp-cli` cannot resolve until `vowp-core` is visible.

## Crate name, binary name and the `vowp` meta-crate

The binary is defined in the root `Cargo.toml`:

```toml
[[bin]]
name = "vowp"
path = "crates/vowp-cli/src/main.rs"
```

The package is `vowp-cli`, so the install command is:

```sh
cargo install vowp-cli
vowp --version
```

From a checkout, install from the workspace root with `cargo install --path .`; the source still lives
in `crates/vowp-cli/src`.

**Decision: no `vowp` meta-crate is published for 0.1.0.** A crate that only depends on `vowp-cli` would
not make `cargo install vowp` work, because `cargo install` installs binary targets of the selected package,
not binaries from its dependencies. A `vowp` crate would only be useful if it duplicated the binary target
(another `src/main.rs` calling `vowp_cli::run`). That adds a second binary with the same name and a second
thing to keep in step for no user-visible gain while `cargo install vowp-cli` already installs `vowp`.
`docs/design/plan.md` mentions `cargo install vowp` as a later option; that remains a possible future
change, not a 0.1.0 deliverable. The name `vowp` is left unpublished.

## Metadata policy

Each crate `Cargo.toml` declares:

- `description`, `license.workspace = true`, `repository.workspace = true`,
  `homepage.workspace = true`, `documentation = "https://docs.rs/<crate>"`, `readme = "README.md"`.
- Up to five `keywords` and up to five crates.io `categories`.
- `include = ["src/**", "README.md", "Cargo.toml"]` for the library crates. The root `vowp-cli`
  manifest includes `crates/vowp-cli/src/**`, the three embedded framework directories and the crate
  README.

The README in each crate describes that crate, says it is part of VOWP, links the main VOWP README, and
states that 0.1.0 is an early release.

### What is in a package, and what is not

Published packages contain the crate's source, its README and the generated Cargo manifest. The
`vowp-cli` package contains `crates/vowp-cli/src/**`, `crates/vowp-cli/README.md` and the embedded
`framework/templates`, `framework/skills` and `framework/opencode` files.

Integration tests are deliberately **not** published. Several test fixtures contain local paths and
recorded process output, and the tests are not needed to build the crate. Cargo therefore prints
`warning: ignoring test ... as tests/... is not included in the published package` during packaging. That
warning is expected and is not a failure.

`cargo package --list` output also contains `Cargo.lock`, `Cargo.toml.orig` and `.cargo_vcs_info.json`.
Cargo generates those files itself; they are normal package metadata.

### Why the `vowp-cli` manifest is at the workspace root

`vowp-cli` embeds the framework templates with `include_str!("../../../framework/...")`. A crates.io
package can only contain files under its own package directory, so a manifest in `crates/vowp-cli`
would leave those assets outside the package and `cargo package` would fail to compile the packaged
crate. The publishable manifest therefore lives in the root `Cargo.toml`, with the source in
`crates/vowp-cli/src` and `framework/` inside the package root. The normalized package manifest records
the `crates/vowp-cli/src` paths; `cargo install vowp-cli` builds the same `vowp` binary.

The `version` integration test is wired up as `scripts/release/version-test.rs` because the release
job's owned paths do not include `crates/vowp-cli/tests/version.rs`. It performs the same two assertions
(binary output and the pinned package version).

## Version bump procedure

The release version is `0.1.0`. To prepare the next version (for example `0.2.0`):

1. Edit the workspace version in the root `Cargo.toml`:

   ```toml
   [workspace.package]
   version = "0.2.0"
   ```

2. Update every inter-crate `version = "0.1.0"` in the dependent crates to the same new version. The
   root `vowp-cli` package dependencies and the six library manifests are the places to check:

   ```text
   Cargo.toml                         (the vowp-cli package)
   crates/vowp-store/Cargo.toml
   crates/vowp-graph/Cargo.toml
   crates/vowp-adapters/Cargo.toml
   crates/vowp-core/Cargo.toml
   ```

3. Update the version assertion in `scripts/release/version-test.rs`. It derives the expected output from
   `CARGO_PKG_VERSION` and then pins the release version so a bump cannot be forgotten.

4. Refresh `Cargo.lock` with a normal workspace build, then commit the lockfile. Do not hand-edit it:

   ```sh
   cargo check --workspace
   git add Cargo.lock
   ```

5. Update the version strings outside this job's paths (main README, `CHANGELOG.md`, release notes, npm
   package and docs) as part of the release job. The crate manifests and crate READMEs are the only ones
   this procedure owns.

6. Run the full pre-flight below. Do not publish a version that fails any step.

Version numbers already on crates.io are immutable. You cannot replace or delete a published version; you
can only yank it. Choose the next version instead.

## Pre-flight checklist

Run from the release commit, in this order:

```sh
git status --porcelain
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
scripts/release/check-packages.sh
scripts/release/publish-crates.sh
```

Expected results:

- `git status --porcelain` prints nothing. `publish-crates.sh --execute` refuses a dirty tree.
- `cargo fmt --all -- --check` exits 0.
- `cargo clippy --workspace --all-targets -- -D warnings` exits 0.
- `cargo test --workspace` passes.
- `check-packages.sh` ends with `RESULT: PASS (7 crates packaged and verified)` and its table shows
  `ok` for both the file list and the publish dry run of every crate.
- `publish-crates.sh` repeats those checks and prints the planned publish order without uploading.

Also check the metadata that will appear on crates.io:

```sh
cargo metadata --no-deps --format-version 1 | python3 -m json.tool | grep -E '"name"|"version"|"description"|"documentation"'
```

And confirm that the manifests and crate READMEs contain no pre-release version suffix:

```sh
grep -R "0\.1\.0-" Cargo.toml crates/*/Cargo.toml crates/*/README.md
```

That command should print nothing after the version bump. (Historical release notes under
`docs/release/` and `CHANGELOG.md` are intentionally excluded from this check.)

## Dry-run behaviour and the local sibling patches

A first release has a bootstrap problem: `cargo publish --dry-run -p vowp-store` cannot resolve
`vowp-protocol = "0.1.0"` because that version is not on crates.io yet. The raw command fails with:

```text
error: failed to prepare local package for uploading
Caused by:
  no matching package named `vowp-protocol` found
  location searched: crates.io index
```

`cargo package --no-verify` still resolves dependencies in the current Cargo, so it fails the same way.
`scripts/release/check-packages.sh` therefore passes local `[patch.crates-io]` entries to Cargo for the
unpublished siblings:

```sh
cargo publish --dry-run -p vowp-store --allow-dirty \
  --config 'patch.crates-io.vowp-protocol.path="crates/vowp-protocol"'
```

The patch is only used to resolve the build locally. The packaged `Cargo.toml` still records the registry
dependency (`vowp-protocol = { version = "0.1.0", ... }`), and the dry run still aborts before uploading.
`vowp-protocol` and `vowp-tmux` have no unpublished siblings and are checked without a patch. This is a
stronger check than `--no-verify`: the packaged crate is actually compiled.

The script never runs `cargo publish` without `--dry-run`; only `publish-crates.sh --execute` does.

## Publishing

The recommended path is the script:

```sh
CARGO_REGISTRY_TOKEN=... scripts/release/publish-crates.sh --execute
```

It publishes `vowp-protocol`, waits until that version is visible in the crates.io sparse index, then
continues with `vowp-store`, `vowp-graph`, `vowp-tmux`, `vowp-adapters`, `vowp-core` and `vowp-cli`. It
stops at the first failure and prints a status table. The wait timeout is 600 seconds by default; override
it with `VOWP_INDEX_WAIT_SECS` and the poll interval with `VOWP_INDEX_POLL_SECS`.

To run the steps by hand, in this exact order:

```sh
cargo publish -p vowp-protocol
# wait until the index has it, then:
cargo publish -p vowp-store
cargo publish -p vowp-graph
cargo publish -p vowp-tmux
cargo publish -p vowp-adapters
cargo publish -p vowp-core
cargo publish -p vowp-cli
```

After each command, check the index before starting the next crate:

```sh
curl -fsS https://index.crates.io/vo/wp/vowp-protocol | grep '"vers":"0.1.0"'
curl -fsS https://index.crates.io/vo/wp/vowp-store    | grep '"vers":"0.1.0"'
curl -fsS https://index.crates.io/vo/wp/vowp-graph    | grep '"vers":"0.1.0"'
curl -fsS https://index.crates.io/vo/wp/vowp-tmux     | grep '"vers":"0.1.0"'
curl -fsS https://index.crates.io/vo/wp/vowp-adapters | grep '"vers":"0.1.0"'
curl -fsS https://index.crates.io/vo/wp/vowp-core     | grep '"vers":"0.1.0"'
curl -fsS https://index.crates.io/vo/wp/vowp-cli      | grep '"vers":"0.1.0"'
```

The sparse-index URL layout is `{first two}/{next two}/{name}` for names of four or more characters; the
script computes it for each crate. The index can lag the upload by a few seconds.

### Token handling

- The only supported token source is the `CARGO_REGISTRY_TOKEN` environment variable. Do not pass
  `--token` on the command line (it would land in shell history) and do not commit a `.cargo/credentials`
  file.
- `publish-crates.sh --execute` refuses to start when the variable is empty or the working tree is dirty.
- The token value is never printed. A leaked token must be revoked on crates.io and replaced; published
  versions cannot be unpublished.

## Post-publish verification

These steps can only run after a real publish. They are listed here so the release checklist can copy
them; they were **not** run on 2026-11-09 because nothing was published.

```sh
cargo install vowp-cli --version 0.1.0 --force
vowp --version
cargo info vowp-protocol
```

In a clean container or a temporary `CARGO_HOME`, confirm that `cargo install vowp-cli` installs a binary
named `vowp` and that `vowp --version` prints `vowp 0.1.0`. Then open one docs.rs page, for example
`https://docs.rs/vowp-protocol`, and confirm it built.

## Rollback

- A published version cannot be deleted or replaced. `cargo yank --version 0.1.0 <crate>` only stops new
  dependency resolutions from selecting it.
- If a crate is broken, publish a fixed version (for example `0.1.1`) and yank the broken one. Do not
  reuse the version number.
- If a crate was published before its dependencies, the later crates cannot resolve. Publish the missing
  dependency first; the already-published crate can stay as-is if its manifest is correct, or be yanked
  and re-published under a new version if it is not.

## Evidence from the preparation run (2026-11-09)

The checkout was at branch `job/p04-packaging-crates` with workspace version `0.1.0`. The checks printed:

```text
vowp-protocol   27 files  list ok  publish dry run ok
vowp-store      22 files  list ok  publish dry run ok
vowp-graph      14 files  list ok  publish dry run ok
vowp-tmux       11 files  list ok  publish dry run ok
vowp-adapters   19 files  list ok  publish dry run ok
vowp-core       54 files  list ok  publish dry run ok
vowp-cli        42 files  list ok  publish dry run ok  (crates/vowp-cli/src + framework assets)
RESULT: PASS (7 crates packaged and verified)
```

`scripts/release/publish-crates.sh` (dry run) printed the same table and the planned publish order, then
`DRY RUN COMPLETE: no crate was published.` Running it with `--execute` and no token refused before any
cargo publish call; running it with a dummy token and a dirty tree refused because the tree was dirty.
No `cargo publish` without `--dry-run` was run and no crates.io login was performed.
