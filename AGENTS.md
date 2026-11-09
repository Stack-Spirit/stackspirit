# AGENTS.md — rules for AI agents working in this repository

This repository is built largely by AI agents (Claude Code and opencode) under a supervisor that gives each
agent one **job**: a brief, a set of owned paths, gates, and a report. These rules apply to every agent and
every harness. A job's brief can add rules; it never removes these.

## The job contract

- **Owned paths only.** Change only the paths your brief lists under `owns`. Read anything; write nothing
  else. `Cargo.lock` is the one shared exception: let cargo update it, never edit it by hand.
- **Never push.** Commit in your own worktree/branch. The supervisor's merge queue is the only writer of
  `main`; it rebases, runs the repo-wide checks, and pushes.
- **Work until the report exists.** Do not stop after announcing a next step. If you need the owner, write
  the question under `questions_for_owner` in your report and set `status: blocked`; never wait in chat.
- **Evidence over claims.** Your report states the exact commands you ran and their results. Gates re-run
  them; a verifier reads your diff.
- **Refused or risky actions are staged, never worked around.** Nothing public: no publishing, no deploys,
  no repository visibility changes, no new accounts or keys.

## Rust conventions

- Stable Rust, edition 2024, the toolchain pinned by `rust-toolchain.toml`. Workspace crates live in
  `crates/` (`vowp-protocol`, `vowp-store`, `vowp-graph`, `vowp-tmux`, `vowp-adapters`, `vowp-core`,
  `vowp-cli`).
- Before every commit: `cargo fmt --all`, then `cargo clippy --workspace --all-targets -- -D warnings`
  and `cargo test --workspace` must pass.
- Add dependencies with `cargo add -p <your-crate> <dep>` so versions are real and current; add them only
  to the crate you own. Prefer the dependencies already declared in the workspace.
- Libraries return typed errors (`thiserror`); `anyhow` only in the `vowp` binary. No `unwrap()`/`expect()`
  in library code outside tests unless the invariant is documented at that line. No `unsafe`.
- Prefer plain threads and blocking I/O; use `tokio` only where a stream genuinely needs it.
- Public items get doc comments. Keep modules small and names plain.

## Tests are real and hermetic

- A test must fail if the code is wrong. No tests that only assert `true`, no hard-coded passes, no
  `todo!()`/`unimplemented!()` presented as features (stubs only where a brief explicitly allows them).
- Tests never touch the real home directory, `~/.vowp`, the user's tmux server, the network, or real
  `claude`/`opencode` binaries. Use temporary directories, a **private tmux socket** (`tmux -L <unique>`)
  **and** a private `TMUX_TMPDIR`, unset `TMUX`/`TMUX_PANE`, recorded fixtures, and fake harness scripts.
- Never run `tmux kill-server`, `tmux -L default ...`, or any tmux command without a private
  `-L`/`TMUX_TMPDIR`, in tests, scripts or manual reproductions. A test or script must refuse `default`
  before spawning tmux.

## Secrets

- Never read, print, log or commit secret values (API keys, tokens, `.env` files, SSH keys). Refer to
  secrets by **name** only. A secret-scan gate runs before every push.

## Commits

- Small, focused commits with a clear subject line (`crate: what changed`).
- End every commit message with the trailer `AI-Assisted: <model> via vowp-night`
  (for example `AI-Assisted: deepseek/deepseek-flash#max via vowp-night`).

## Documentation

- Docs describe what the code actually does. Every command shown must run. No invented features; mark
  planned features as planned.
