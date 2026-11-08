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
   real harness binaries. Use temporary directories, a private tmux socket (`tmux -L <unique>`), and fake
   harness scripts.

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
