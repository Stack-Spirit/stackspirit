# scripts/acceptance — the reusable VOWP acceptance rig

This directory is the T0 rig for the v0.2 acceptance jobs (T1–T4). It gives
each test a disposable **fresh VPS** with sshd, an optional **no-sudo** variant,
a second SSH client that plays the **phone**, two fixture projects, and a
generator for a **v0.1.0 home with real history**.

Everything runs in Docker on the local machine. A test never touches the real
`~/.vowp`, the host tmux server, the network (apart from image build and the
one-time v0.1 release download), or a real `claude`/`opencode` binary.

## Contents

| Path | Purpose |
|---|---|
| `rig.sh` | Build, start, inspect and remove the containers; run fixtures and the v0.1 generator. |
| `lib.sh` | Host-side helpers: temporary HOME, private `TMUX_TMPDIR`, tmux wrapper, SHA-256. |
| `container/Dockerfile` | Ubuntu 24.04 "fresh VPS" with sshd, git, tmux, curl, python3 and a `vowp` user. `WITH_SUDO=0` removes sudo. |
| `container/phone/Dockerfile` | SSH-client-only container with its own key and the same private tmux wrapper as the home image. |
| `fixtures/fake-worker.sh` | Deterministic fake harness; normal and memory-hog modes; also answers verifier prompts. |
| `fixtures/project.sh` | Creates a fixture git checkout, a bare local origin, and a fake harness adapter. |
| `make-v01-home.sh` | Downloads the published v0.1.0 release, verifies its SHA-256, runs the real v0.1 supervisor with a fake harness, and records a manifest. |
| `tests/run.sh` | End-to-end self-test for the rig itself. |

## Requirements

- Docker with a reachable daemon.
- `bash`, `ssh`, `ssh-keygen` on the host.
- For the direct-host `fixtures/project.sh` recipe: `git`.
- For the direct-host `make-v01-home.sh` recipe: `tmux`, `git`, `python3`, and
  `sha256sum` (or `shasum`), plus `tar` and `curl` or `wget` when it downloads
  a release; with `--binary` only the first four are used. The home image
  contains these; the phone image is an SSH-client image without them, so run
  the recipe on the host or in the home container.
- The self-test's host-side helper check also needs `tmux` and `sha256sum` (or
  `shasum`) on the host; its container checks use the tools in the images.
- Network access while building the images (base image and apt packages) and,
  unless you pass `--binary`, while generating a v0.1 home (published release).

No root, no sudo, no host key, no host tmux.

## Quick start

```sh
bash scripts/acceptance/rig.sh build
bash scripts/acceptance/rig.sh up --name demo
eval "$(bash scripts/acceptance/rig.sh env --name demo)"

bash scripts/acceptance/rig.sh ssh --name demo 'uname -a'
bash scripts/acceptance/rig.sh phone --name demo 'echo this runs on the home'
bash scripts/acceptance/rig.sh exec --name demo 'echo this is a docker exec, as vowp'

bash scripts/acceptance/rig.sh down --name demo
```

`down --keep-state` keeps the generated keys and state file; a later `up`
reuses them instead of generating new keys. After the `eval`, `RIG_NAME` makes
`--name` optional; the commands here pass it explicitly.

`ssh` runs a command on the home through sshd using the owner key. `phone` runs
a command inside the phone container and then over a second SSH connection to
the home; a bare `rig.sh phone` opens the phone's SSH session. The fresh-VPS
containers have no VOWP binary until the test installs one.

## What the containers give you

### Home ("fresh VPS")

- Ubuntu 24.04, user `vowp` (uid 1001), hostname `vps`, sshd on port 22.
- `sudo` with passwordless NOPASSWD for `vowp` in the default variant.
- `no-sudo` variant: no `sudo` binary and no sudoers entry.
- git, tmux, curl, python3, sqlite3, jq, rsync, openssh-client, openssh-server.
- No VOWP source or binary, no `~/.vowp`, no harness logins.
- SSH sessions get `TMUX_TMPDIR=/home/vowp/.acceptance/tmux` and a private
  `VOWP_ACCEPTANCE_TMUX_SOCKET` through sshd `SetEnv`. `/usr/local/bin/tmux` is
  a wrapper that refuses the name `default`, an empty/shared `TMUX_TMPDIR`, and
  an explicit `-S <path>`, and adds `-L <private>` when a call omits one.

The rig mounts `scripts/acceptance` read-only at `/opt/vowp-acceptance` so tests
can run `fixtures/project.sh` and `make-v01-home.sh` inside the VPS without
copying the product source tree there.

### Phone

A second container with only an SSH client. It has its own generated key, an
`ssh_config` naming `home`, the same private tmux wrapper and `TMUX_TMPDIR`
guard as the home image, and no relationship to the host SSH agent. Both the
owner and phone keys are installed in the home's `authorized_keys`. Use it for
the phone surface, lease/takeover checks and SSH-drop scenarios:

```sh
bash scripts/acceptance/rig.sh phone --name demo 'hostname; id -un'
bash scripts/acceptance/rig.sh phone --name demo 'printf "%s\n" "phone surface"'
```

## Fixture projects

`fixture` creates an ordinary git checkout with a committed fake harness and a
bare local `origin` next to it. It does not register the project with a home;
the v0.2 test registers it with `vowp project add` (planned for C2) or the
command under test.

```sh
# A small project whose worker writes src/<job>.txt and a report.
bash scripts/acceptance/rig.sh fixture --name demo hello /home/vowp/projects/hello

# A project whose worker allocates 512 MiB, holds it for 60 s, then finishes.
bash scripts/acceptance/rig.sh fixture --name demo memory-hog /home/vowp/projects/hog \
    --hog-mb 512 --hog-hold 60

# Both at once, for two-project runs.
bash scripts/acceptance/rig.sh fixture-pair --name demo /home/vowp/projects/pair
```

Each fixture contains `.acceptance/seed-jobs.sh`:

```sh
bash scripts/acceptance/rig.sh ssh --name demo '
cd /home/vowp/projects/hello &&
.acceptance/seed-jobs.sh 3 &&
vowp up --detach
'
```

`seed-jobs.sh [count] [prefix]` runs `vowp job add` (the `VOWP` environment
variable can name a binary) with `report,owns_respected` gates. The worker
writes exactly one owned file, commits it, and writes its report. For
`memory-hog`, each worker allocates its configured MiB, touches every page,
holds it for the configured seconds, then writes the report, so capacity and
admission tests can observe real memory.

The fixture's normal worker also answers verifier prompts with a passing
verifier report. This lets a test exercise the real review gate without a model.

## v0.1 home generator

Generate a real v0.1 home inside the rig:

```sh
bash scripts/acceptance/rig.sh v01-home --name demo \
    --dest /home/vowp/acceptance/v01 --jobs 2

# Or run the generator directly on a Linux/macOS host:
bash scripts/acceptance/make-v01-home.sh --dest /tmp/v01-home --jobs 2

# Offline / specific binary (must report `vowp 0.1.0`):
bash scripts/acceptance/make-v01-home.sh --dest /tmp/v01-home \
    --binary /path/to/vowp-0.1.0
```

The generator:

1. downloads `vowp-0.1.0-<target>.tar.gz` and `SHA256SUMS` from the published
   GitHub release, verifies the archive SHA-256, and checks the exact
   `vowp 0.1.0` version; with `--binary` it checks the same exact version and
   records the binary's SHA-256 in the manifest, but there is no published
   checksum to compare against in that mode;
2. creates `<dest>/home` (`VOWP_HOME`), `<dest>/project` (registered at its
   absolute path), and `<dest>/origin.git`;
3. writes `<dest>/manifest.json` with the binary version and SHA-256, schema
   version, job states, table row counts, session logs and run files;
4. runs the real v0.1 supervisor with the fake harness until every job is
   `done`, then `vowp down --force`;
5. uses `<dest>/runtime` for its own `HOME`, `TMPDIR` and tmux wrapper; the
   v0.1 binary's own `Tmux::for_home` uses `<dest>/home/tmux`, which the
   generator also kills on exit.

The project path is recorded in the v0.1 database, so use the generated
directory in place. Migration is planned for C2; once that command exists,
point the v0.2 binary at the same home:

```sh
# planned for C2
vowp --home /home/vowp/acceptance/v01/home migrate
```

No real harness, model, secret or network is involved after the download.

## Acceptance-job recipes

The card T0 exists so the later tests can be short and repeatable.

| Test | Rig usage |
|---|---|
| **T1 fresh setup** | `up --variant sudo` and `up --variant no-sudo`; run the README prompt over `ssh`; the phone joins with `phone`. The image starts with no VOWP and no `~/.vowp`. |
| **T2 two projects** | `fixture-pair`, register both projects on one home with `vowp project add` (planned C2), seed jobs, run `vowp up`, and assert neither starves. |
| **T3 capacity** | `fixture memory-hog --hog-mb … --hog-hold …`, start several jobs, sample `capacity report` (planned E2), and check the brake stops admissions without killing a live worker. `up --memory 1g` also sets a Docker memory limit. |
| **T4 surfaces** | Use `ssh` for the owner and `phone` for the second SSH surface; `phone` keeps its own key and connection. `ssh --as phone` exercises the phone key from the host too. |
| **SSH drop** | Run the client through `phone`, then close the phone container or `docker exec ... pkill ssh`; the home and its files stay untouched. |
| **T6 migration** | `v01-home` creates the v0.1 home with real history; `manifest.json` gives the pre-migration row counts. The `vowp migrate` command is planned for C2. |
| **T7/T8 GC + secrets** | Use any fixture project, put the home under a temporary `VOWP_HOME`, and point the test's logs/report paths inside the container. |

## Command reference

```text
rig.sh build  [--variant sudo|no-sudo|all] [--no-cache]
rig.sh up     [--name NAME] [--variant sudo|no-sudo] [--memory SIZE] [--no-build]
rig.sh down   [--name NAME] [--keep-state]
rig.sh status [--name NAME]
rig.sh env    [--name NAME]
rig.sh ssh    [--name NAME] [--as owner|phone] [--] <command...>
rig.sh phone  [--name NAME] [--] <command...>
rig.sh exec   [--name NAME] [--] <command...>
rig.sh logs   [--name NAME] [--tail N]
rig.sh fixture      [--name NAME] <hello|memory-hog> [<dest>] [fixture options]
rig.sh fixture-pair [--name NAME] [<dir>]
rig.sh v01-home     [--name NAME] [make-v01-home options]
rig.sh selftest     [test options]
rig.sh list
```

Useful environment variables:

| Variable | Meaning |
|---|---|
| `VOWP_ACCEPTANCE_STATE_DIR` | Root for generated keys, state files and rig metadata (default `${TMPDIR:-/tmp}/vowp-acceptance`). |
| `VOWP_ACCEPTANCE_VPS_IMAGE` | Base image tag for the VPS variants (default `vowp-acceptance/vps`). |
| `VOWP_ACCEPTANCE_PHONE_IMAGE` | Phone image tag (default `vowp-acceptance/phone:local`). |
| `VOWP_V010_BASE_URL` | Release download base URL (forwarded by `rig.sh v01-home`). |
| `VOWP_V010_CACHE` | v0.1 archive cache (default `${TMPDIR:-/tmp}/vowp-acceptance-cache`). |
| `VOWP_V010_TARGET` | Override the release archive target (forwarded by `rig.sh v01-home`). |
| `RIG_*` | Values printed by `rig.sh env`; use them in test scripts. |

`rig.sh env` prints shell-quoted `export` lines for `RIG_HOME_CONTAINER`,
`RIG_PHONE_CONTAINER`, `RIG_SSH_PORT`, `RIG_OWNER_KEY`, `RIG_PHONE_KEY`,
`RIG_USER_HOME`, `RIG_HOME`, `RIG_TMUX_TMPDIR`, `RIG_TMUX_SOCKET` and the rest
of the state. `RIG_USER_HOME` is the Unix home (`/home/vowp`); `RIG_HOME` is the
VOWP home (`/home/vowp/.vowp`).

## Self-test and evidence

Run the full rig self-test (builds images, starts both variants, checks SSH,
sudo/no-sudo, phone, tmux privacy, fixtures and the v0.1 generator):

```sh
bash scripts/acceptance/tests/run.sh
```

It prints each stage and `PASS` only when every assertion passed. Useful:

```sh
bash scripts/acceptance/tests/run.sh --no-build   # reuse built images
bash scripts/acceptance/tests/run.sh --skip-v01   # no release download
bash scripts/acceptance/tests/run.sh --keep       # keep rigs for inspection
```

The test uses only containers and temporary state. On failure it leaves the
named rigs up and prints their names; remove them with
`bash scripts/acceptance/rig.sh down --name <name>`. `rig.sh list` shows the
home containers; `rig.sh status --name <name>` shows both.

## Safety rules this rig enforces

- The host `~/.vowp`, real HOME and tmux server are never used. The host
  helpers unset `TMUX`/`TMUX_PANE` and write a wrapper that refuses the default
  server before it can be started.
- Destructive fixture/generator destinations must be strict children of
  `/tmp`, `/var/tmp` or a non-system `$HOME`. Newlines/control characters and
  symlinked parents are refused, and a misconfigured HOME (for example
  `HOME=/etc`) cannot widen the allowlist.
- tmux refuses a missing `TMUX_TMPDIR`; both the host library and the image
  create the directory first, because tmux silently falls back to the default
  server when it is absent.
- Keys are generated per rig under the state directory (mode 0600/0700) and
  removed by `down` unless `--keep-state` is passed. No key is committed.
- The containers have no host home mount. The home receives generated public
  keys and the read-only `scripts/acceptance` directory. The phone receives its
  own generated private key and SSH config through a read-only mount.
- The phone has its own key and its own container; it is not the host SSH
  agent. It also has a private `TMUX_TMPDIR` and the same wrapper as the home,
  so a phone-side tmux call cannot reach that container's default server. The
  generated phone private key is part of the disposable state and is removed by
  `down`.
- Images contain no VOWP binary and no harness login. The fake harness never
  starts `claude` or `opencode` and never opens a socket or a model turn.
- Docker build and the v0.1 release download are the only network operations.
  The self-test's `--skip-v01` and `--no-build` flags remove both when needed.

## Limits and ownership

- The rig is Linux-container based; the host scripts are bash and avoid
  GNU-only tools such as `seq`, so the intended macOS path uses the same
  commands, but the self-test itself runs on Linux. macOS-specific product
  code is not exercised by the rig (the repo's CI covers macOS).
- The no-sudo variant removes `sudo` only; it does not emulate a systemd-less
  host beyond the container's normal lack of a running init system.
- All paths in the v0.1 home are absolute. Use the generated directory in
  place or move `home/` and `project/` together and update the project row.
- This directory is internal (`scripts/acceptance/**`). The public-tree audit
  must add `prune scripts/acceptance/` before publication; it is not yet in
  `scripts/audit/public-tree.txt`, so do not assume it is automatically
  excluded. Keep hostnames, owner paths and keys out of it.
