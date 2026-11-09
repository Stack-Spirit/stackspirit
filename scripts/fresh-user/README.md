# scripts/fresh-user — follow the docs as a new user

`run.sh` is the fresh-user acceptance test. It builds a clean Rust container,
copies this checkout in without `target/` or `.git`, and runs the documented
commands from:

- `README.md`, section **Quickstart (from source)**;
- `docs/guides/install.md`, sections **Prerequisites** through **Verify the node**;
- `docs/guides/quickstart.md`, sections 0 through 5;
- `examples/hello-vowp/README.md`, the main **Run it** command.

The commands are extracted from the fenced `sh` blocks at run time by
`extract.py`, so the test follows the docs instead of keeping a second copy of
them. A missing section or block is an extraction error, not a silent skip.

## Run it

```sh
bash scripts/fresh-user/run.sh
```

Requirements: `docker` with a reachable daemon, and `python3` on the host. The
container is the official `rust:1-slim-bookworm` image plus `git` and `tmux`.
It starts with no VOWP binary and no build cache. `run.sh` prepares the Docker
build context (`repo/`, `fresh-user-plan.sh`, `fake-bin/`), so build the image
through `run.sh` rather than pointing `docker build` at this directory.

Every command is one timed step. The script stops at the first non-zero exit and
prints the document and line the command came from. It also checks the outputs
the docs promise: `vowp 0.1.0-dev`, the `vowp init` summary line, `doctor: ok`,
and the hello-vowp example's final "all three jobs reached done".

The container gets no host environment and no secrets. Two fake harnesses
(`fake-bin/claude` and `fake-bin/opencode`) answer only `--version`, because the
container cannot sign in to a real harness; `vowp doctor` would otherwise fail
its documented harness check. The hello-vowp example uses its own fake worker
and runs the real supervisor, gates, worktrees and merge queue.

## Options

| Variable | Meaning |
|---|---|
| `FRESH_USER_IMAGE=tag` | Image tag to build and run (default `vowp-fresh-user:local`). |
| `KEEP_IMAGE=1` | Keep the image after the run. |
| `KEEP_WORK=1` | Keep the temporary build context, plan and log. |

By default the container and the image are removed. Docker's layer cache may
remain; that is shared build cache, not test state.

## What is deliberately skipped

`extract.py` skips commands the docs themselves mark `# (planned)` in the
quickstart guide (currently only `vowp plan --dry-run`, which needs a planner
model turn). The optional `KEEP=1` inspection variant of the hello-vowp example
is also skipped; the main `FAKE=1` command is the one that runs.

`docs/fresh-user/REPORT.md` records the deviations found by the last run.
