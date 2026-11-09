# hello-vowp — a three-job VOWP demo

This example runs a complete VOWP graph in about a minute, without a model, network access,
or a real harness. It uses the generic `cli` adapter with a tiny fake worker script; the
supervisor, sessions, gates, git worktrees, dependency edges and merge queue are the real
ones.
The graph has three jobs:

```
hello-greeting  ─┐
                 ├─> hello-message
hello-farewell  ─┘
```

Each job writes one file under `src/`, passes a `report` gate, an `owns_respected` gate and
a content-checking command gate, and is merged into `main` by the merge queue.

## Prerequisites

- Rust (the toolchain from the repository's `rust-toolchain.toml`) and `git`.
- `tmux` is **not** needed: the script starts the supervisor with `--no-tmux`. Because
  `vowp up` still ensures a window layout, the script points `tmux` at a private socket
  directory under the temporary work directory and kills that server at the end; it never
  contacts the caller's tmux server.

## Run it

From the repository root:

```sh
FAKE=1 examples/hello-vowp/run.sh
```

`run.sh`:

1. creates a temporary directory with a bare `origin` and a demo checkout;
2. creates a temporary VOWP home and runs `vowp init` there;
3. adds the three jobs with `vowp job add`;
4. starts `vowp up --detach --no-tmux` with a fake `cli` adapter;
5. polls until `hello-greeting`, `hello-farewell` and `hello-message` are all `done`;
6. runs `vowp down` and checks the merged files in the checkout and on `origin/main`.

It exits 0 only when all three jobs reached `done` and the merged content is correct. It
never touches `~/.vowp`, the real tmux server, the network, or a real `claude`/`opencode`
binary. The temporary directory is removed on success.

Useful variables:

| Variable | Meaning |
|---|---|
| `VOWP=/path/to/vowp` | use a specific `vowp` binary (otherwise an installed/built binary, or `cargo run`) |
| `TIMEOUT=300` | seconds to wait for the jobs (default 180) |
| `KEEP=1` | keep the temporary home and repository for inspection |
| `WORK=/some/dir` | use an explicit work directory instead of `mktemp` |

Example with a kept work directory:

```sh
KEEP=1 FAKE=1 examples/hello-vowp/run.sh
```

Then inspect the result with the binary and `--home`/`--project` printed by the script:

```sh
vowp --home /tmp/hello-vowp.XXXX/home --project /tmp/hello-vowp.XXXX/hello-vowp status
vowp --home /tmp/hello-vowp.XXXX/home --project /tmp/hello-vowp.XXXX/hello-vowp board --once
```

## Files

| Path | Purpose |
|---|---|
| `run.sh` | the runnable acceptance script |
| `vowp.toml` | fake-adapter project config copied into the temporary checkout |
| `mapping.toml` | generic `cli` adapter mapping for the fake worker |
| `fake-worker.sh` | the fake harness: writes the job file, commits it, writes the report |
| `briefs/*.md` | brief bodies for the three jobs |
| `project/` | the initial demo repository contents |

The fake worker is deliberately small: it reads the report path from the L2 prompt, writes
the file named for the job, commits it, and emits a report with `status: done`. The gates
and merge queue below it are not faked.

## Why fake adapters?

This example is for checking the wiring and watching the lifecycle without spending model
tokens or configuring a harness. For a real run, replace the `[profiles.*]` ladders and the
`[harnesses.fake-worker]` entry with a real `claude-code`, `opencode`, or `cli` mapping and
keep the same graph and gates.
