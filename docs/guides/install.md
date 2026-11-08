# Install

VOWP is a single Rust binary. In v0.1 you build it from source; prebuilt release archives, an
`install.sh`, Homebrew and an npm wrapper are planned for the first public release.

> **Status: planned for the CLI.** The build commands below work today. Commands that start with `vowp`
> are *(planned)* until the CLI jobs merge; the current binary only implements `vowp --version`.

## Prerequisites

| Requirement | Why |
|---|---|
| Linux (x86_64/arm64) or macOS (arm64/x86_64) | The v0.1 platforms. |
| Rust from [`rust-toolchain.toml`](../../rust-toolchain.toml) | The workspace pins a stable toolchain and edition 2024. With `rustup`, any `cargo` command uses it automatically. |
| `git` | Worktrees, branches, the merge queue and release checks. |
| `tmux` | The window manager. The supervisor can run without it (`--no-tmux`), but then there are no live windows. |
| A harness such as [Claude Code](https://code.claude.com/docs/en/overview) or [opencode](https://opencode.ai/docs/) | The agent that actually does the work. Install and sign in with the harness's own tooling. |

Check the basics:

```sh
git --version
tmux -V
cargo --version
rustc --version
```

VOWP does not need a provider key of its own and never reads a harness login. If a harness needs an
environment variable (for example a provider key), set it in your shell exactly as you already do for
that harness. `vowp doctor` (planned) reports only whether the **name** is present; it never prints the
value and never calls the network. See the [security model](../security.md).

## Build from source

From the repository root:

```sh
cargo build --workspace
cargo run -p vowp-cli --bin vowp -- --version
```

The last command prints:

```
vowp 0.1.0-dev
```

For a release build:

```sh
cargo build --release --workspace
```

## Put `vowp` on your `PATH`

The binary package is `vowp-cli`; the executable is `vowp`.

```sh
cargo install --path crates/vowp-cli
vowp --version
```

`cargo install` writes to `~/.cargo/bin` by default. If that directory is not on your `PATH`, add it in
your shell profile, or pass `--root <dir>` and add `<dir>/bin` instead.

## Prepare a project repository

VOWP manages an existing git repository; it does not create one for you. The project must have a
default branch and, for the merge queue, a remote it can fetch and push to. If you only want to try the
supervisor on a local repository, an `origin` remote is still required by the merge path.

The first project command is:

```sh
# (planned)
vowp init --name my-project
```

It writes `vowp.toml`, installs the framework files from [`framework/`](../../framework/README.md) into the
repository, appends `.vowp/tmp/` to `.gitignore`, creates `~/.vowp`, registers the local node and project,
and seeds the default loop rows. `vowp up` also ensures the loop rows on startup, so an existing project
picks up the current defaults. Existing files are skipped unless you pass `--force`.

## Verify the node

```sh
# (planned)
vowp doctor
```

`doctor` checks, with `ok`, `warn` or `fail` per item:

- the VOWP home is writable and the database opens, passes `integrity_check` and has a supported schema;
- `git` and `tmux` versions;
- harness probes for `claude` and `opencode`;
- `vowp.toml` parses and validates, and every profile ladder resolves to an installed harness;
- provider environment variable **names** are present or absent (never values, never a network call);
- the systemd user unit or cron watchdog is installed (warn only);
- a supervisor is running (warn only);
- at least 1 GB of free disk space.

Exit code 0 means no `fail`. A missing harness is a `fail`; a missing tmux is a `warn`.

## Where things go

| Path | Contents |
|---|---|
| `~/.vowp/` | Node state: database, session logs, blobs, runs, logs, backups, inbox copies. |
| `<repo>/vowp.toml` | Project configuration, committed. |
| `<repo>/AGENTS.md`, `CLAUDE.md`, `opencode.jsonc` | Framework files installed by `vowp init` (planned), committed. |
| `<repo>/.vowp/` | Job briefs and reports, checkpoints, optional graph snapshot, release log. |
| `<repo>/../<repo>-wt/<job-id>` | Job worktrees on branch `job/<job-id>`. |

Treat `~/.vowp` as sensitive: session logs can contain prompts, tool output and file contents. Back it
up like any other work data, and never commit it.

Next: [quickstart](quickstart.md).
