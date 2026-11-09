# Contributing to VOWP

VOWP is built in small, owner-scoped jobs. This page covers setup, the build, the checks CI runs, the
safe way to run the tmux tests, and the workflow every human or AI contributor follows.

## Set up

VOWP is a Rust workspace. The workspace pins a stable toolchain in
[`rust-toolchain.toml`](rust-toolchain.toml) and uses edition 2024; with `rustup` installed, any
`cargo` command in this repository uses the pinned toolchain automatically.

1. Install [Rust with rustup](https://rustup.rs/) if you do not have it. `rustup show` prints the
   active toolchain.
2. Install `tmux` with your operating system's package manager. The tests and the supervisor's live
   windows use it; CI installs it with apt on Linux and Homebrew on macOS.
3. Install `git`. Worktrees, branches, the merge queue and releases use it.
4. Optionally install a harness such as [Claude Code](https://code.claude.com/docs/en/overview) or
   [opencode](https://opencode.ai/docs/), signed in with its own tooling, if you want to run the
   supervisor end to end.
5. Fork the repository on GitHub, clone your fork, and create a topic branch for your change.

Check the basics:

```sh
git --version
cargo --version
rustc --version
rustup show
```

VOWP does not need a provider key of its own and never reads a harness login. If a harness needs an
environment variable, set it in your shell exactly as you already do for that harness; `vowp doctor`
reports only whether the **name** is present.

## Build and test

```sh
cargo build --workspace
cargo test --workspace
cargo run -p vowp-cli --bin vowp -- --version
```

The last command prints `vowp <version>`, with `<version>` taken from the workspace `Cargo.toml`.

## Run the test suite safely

Integration tests create their own temporary node home with a private `-L` socket
(`vowp-<hash>` derived from the home in CLI tests, `vowp-test-…` in the tmux crate) and a private
`TMUX_TMPDIR`; they refuse `default` and never read `TMUX` from your shell. `cargo test --workspace`
is safe to run on a machine that also has your own tmux sessions.

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

A test that starts tmux must fail closed: assert that the resolved socket is not the default one
before spawning the server.

## Run the CI checks locally

Run these before every commit; they are the same checks [`.github/workflows/ci.yml`](.github/workflows/ci.yml)
runs on Linux and macOS.

```sh
cargo fmt --all
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## Pull requests and review

1. Open a pull request against `main` from a topic branch in your fork. Fill in the pull request
   template and link the issue it addresses. Draft pull requests are welcome for early feedback.
2. CI must pass: formatting, Clippy with warnings denied, and the test suite on Linux and macOS. A red
   build is not ready for review.
3. A maintainer reviews the change for behavior, tests, documentation, security and tmux safety. Expect
   questions about edge cases; a smaller pull request is easier to review than a broad one.
4. Push follow-up commits to the same branch. Keep the review conversation about the change, not the
   person.
5. A maintainer merges the pull request. In this repository the single merge queue rebases the branch
   and fast-forwards `main`; do not push to `main` directly.
6. Security fixes do not go through a public pull request first. Follow [`SECURITY.md`](SECURITY.md).

Reviewers check, at minimum:

- the change is scoped to the problem and does not include unrelated edits;
- the tests fail without the change and pass with it, and new behavior has a test;
- documentation and `--help` text match the behavior;
- no secret values, personal data or private paths appear in code, tests, logs or examples;
- tests that touch tmux use a private socket and a private `TMUX_TMPDIR`, created before the first
  tmux command;
- commits are focused and carry the trailer described below.

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

## Commit messages

Keep commits small and focused, with a subject like `crate: what changed`. Name the area, use the
imperative mood, and explain **why** in the body when the reason is not obvious. Reference the issue
when there is one. For example:

```text
vowp-tmux: pass the private TMUX_TMPDIR to the attach client

The attach client inherited the caller's TMUX_TMPDIR, so a client started
inside another tmux session could reach the wrong server. Pass the node's
private directory and unset TMUX/TMUX_PANE.
```

End every commit message with the trailer:

```text
AI-Assisted: <model> via vowp-night
```

For example:

```text
AI-Assisted: deepseek/deepseek-flash#max via vowp-night
```

## AI contributors

AI-assisted contributions are welcome and held to the same standard as any other change.

- Label every AI-assisted commit with the `AI-Assisted: <model> via vowp-night` trailer, using the real
  model identifier. Do not invent a model name or omit the trailer.
- Do not claim tests, reviews or benchmarks you did not run. Put the exact commands and their results
  in the pull request or job report.
- Change only the paths the job owns, and keep the diff limited to the problem.
- Never put a secret, token, `.env` value or private path into a prompt, commit, log or test fixture.
- A human submitter is responsible for the pull request and should be able to explain the change and
  its verification.
- Agents working in this repository also follow [`AGENTS.md`](AGENTS.md), including the Rust
  conventions and the hermetic-test rules.

## Documentation

Docs describe what the code actually does. Every command shown must run. Mark planned features as
planned.

## Security

Do not open a public issue for a vulnerability. Follow [`SECURITY.md`](SECURITY.md) and use private
vulnerability reporting.

## Code of conduct

All project spaces follow [`CODE_OF_CONDUCT.md`](CODE_OF_CONDUCT.md). Report unacceptable behavior
privately to the repository owner through GitHub.
