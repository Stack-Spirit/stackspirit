# Contributing to VOWP

VOWP is built in small, owner-scoped jobs. This page covers the build, the checks CI runs, and the
workflow every human or AI contributor follows.

## Prerequisites

- Rust from the pinned toolchain in [`rust-toolchain.toml`](rust-toolchain.toml). With `rustup`
  installed, any `cargo` command in this repository uses it automatically; `rustup show` prints the
  active toolchain.
- `tmux` for the tests and features that drive it (CI installs it with apt on Linux and Homebrew on macOS).

## Build and test

```sh
cargo build --workspace
cargo test --workspace
cargo run -p vowp-cli --bin vowp -- --version
```

The last command prints `vowp 0.1.0-dev`.

**Running the test suite is safe: tests use a private tmux directory.** Integration tests
create their own temporary node home with a private `-L` socket (`vowp-<hash>` derived from
the home in CLI tests, `vowp-test-…` in the tmux crate) and a private `TMUX_TMPDIR`; they
refuse `default` and never read `TMUX` from your shell. If a test ever needs a raw tmux
command, use the same two guards (`-L <unique>` and a private `TMUX_TMPDIR`) and assert the
resolved socket is not the default one.

Rules for every contributor and agent, in tests, scripts and manual reproductions:

- Never run a tmux command against the default server. That includes `tmux kill-server`,
  `tmux -L default kill-server`, and any command run without `TMUX_TMPDIR` set.
- Use a unique socket per run (`tmux -L vowp-…`) **and** a private `TMUX_TMPDIR` under a
  temporary directory. The two layers mean a dropped `-L` still cannot reach your server.
- **Create the private `TMUX_TMPDIR` before the first tmux command.** tmux silently falls
  back to the default server when `TMUX_TMPDIR` names a directory that does not exist.
  `mkdir -p "$private"` first, then run every client with that same directory.
- Unset `TMUX` and `TMUX_PANE` before starting a test harness, script or manual client; a
  caller inside tmux would otherwise point the client at the caller's server.
- `vowp` itself keeps its windows on a private socket derived from the node home and
  records the socket and server pid in `<home>/tmux-server`. `vowp down --kill-windows`
  refuses a server that does not match that record. `vowp attach` passes the same
  `TMUX_TMPDIR` (and unsets `TMUX`/`TMUX_PANE`) to the tmux client it execs.

## Run the CI checks locally

Run these before every commit; they are the same checks [`.github/workflows/ci.yml`](.github/workflows/ci.yml)
runs on Linux and macOS.

```sh
cargo fmt --all
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## Job and ownership workflow

Work is split into jobs. Each job has a brief that lists:

- `owns`: the paths the job may change.
- `must_not_touch`: paths that are off limits.
- `gates`: checks that must pass before the work is accepted.

Rules for every job:

1. Work in your own git worktree and branch. Change only the paths under `owns`; read anything, write
   nothing else. `Cargo.lock` is updated by cargo, never edited by hand.
2. Commit in your worktree. Never push: the supervisor's merge queue is the only writer of `main`; it
   rebases the branch, runs the repository-wide checks and pushes.
3. Work until the report exists. If you need the owner, put the question in the report under
   `questions_for_owner` and mark the job `blocked`; do not wait in chat.
4. Record evidence in the report: the exact commands you ran and their results. Gates re-run them and a
   verifier reads the diff.
5. Keep tests real and hermetic: no network, no real home directory or `~/.vowp`, no user tmux server, no
   real harness binaries. Use temporary directories, a private tmux socket (`tmux -L <unique>`) **and** a
   private `TMUX_TMPDIR`, and fake harness scripts. `-L default` and `tmux kill-server` without a private
   socket are forbidden.

See [`AGENTS.md`](AGENTS.md) for the full rules, including Rust conventions and the secret policy.

## Commits

Keep commits small and focused, with a subject like `crate: what changed`. End every commit message with
the trailer:

```text
AI-Assisted: <model> via vowp-night
```

For example:

```text
AI-Assisted: deepseek/deepseek-flash#max via vowp-night
```

## Documentation

Docs describe what the code actually does. Every command shown must run. Mark planned features as
planned.

## Security

Do not open a public issue for a vulnerability. Follow [`SECURITY.md`](SECURITY.md).
