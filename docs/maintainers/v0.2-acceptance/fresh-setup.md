# v0.2 acceptance — fresh setup (T1)

This is the acceptance record for §13.5 test **1 (a fresh Linux machine set up from zero
by the README prompt, first on the Docker rig and then on one Google Cloud VM)**. It is
the T1 card `v2-t1-fresh-setup`.

**Result on the tested build: both test-1 setup halves pass — the Docker rigs and the
real GCE VM each go from an empty machine to a working home, control machine and
`setup verify`.** This is a qualification, not the literal §13.5 step: neither run used
a real harness pasting the README prompt. That real-harness step was not run and remains
an owner/release check (open item 3). The Docker rig covers the sudo and no-sudo
variants; the one real Google Cloud VM run goes from an empty Ubuntu 24.04 image to a
working home, control machine and `setup verify`, with the supervisor running under the
real systemd user manager and vault-user secret mode. The user-creation path still needs
the accepted one-line workaround for a product defect: `setup home` fails its install
phase when the SSH login user's home is not traversable by the target user (open item 1).
The VM run reproduced that failure on a real GCE image (`sh: 22: cd: can't cd to .`) and
then passed after `chmod 0755` on the test login home.

Three new real-VM observations are open items: `vowp fleet status|queue|inbox` on a
control machine call a bare `vowp` on the home, which a non-login SSH PATH does not
contain (exit 127; `fleet status --cached` still works); the first `setup home` on a
fresh VM warns that it cannot reach the user systemd bus because linger is enabled
after the `systemctl --user` commands (the idempotent re-run starts the unit); and the
read-only plan's harness-login probe runs as the login user, so on a 0750 home it
reports a false `user/login` hand-off even though the apply checks as `vowp` and
passes. The literal README prompt URL still returns **404** on 2026-11-11 because the
`v0.2.0` tag and release are not published yet (J2 is later); the run used the same
contract file from the tested checkout and a loopback release mirror, described under
[What was simulated](#what-was-simulated). The orchestrator deleted the VM and its boot
disk on 2026-11-11 (delete operation at 13:12 UTC); both `gcloud compute instances list`
and `gcloud compute disks list` returned no items. This job did not delete it (see
[Deletion](#deletion-orchestrator)).

## Environment

- Date: 2026-11-11 (UTC). The Docker runs finished at 10:22 and 11:02; the real-VM
  run ran from 12:04 to 12:10.
- Worktree: `job/v2-t1-fresh-setup`, based on `a0d0aee` (`origin/main` at the time),
  rebased onto `e41459f` for the setup-verify fix, and the VM run on `a191c02`
  (product code identical to `e41459f`; the three later commits change only this
  document).
- Docker binary under test: built from the HEAD source with
  `cargo build -p vowp-cli --bin vowp`. The workspace package version is still
  `0.1.0`; because the release tag does not exist, the acceptance copies used for the
  install were built from `git archive HEAD` into temporary trees with the workspace
  version set to `0.2.0` (the J2 version bump; the real-VM build also updated the
  inter-crate version pins). The product code is identical. Its SHA-256 is
  `f610364b327ece5e790145e0ba42b3918e290500e2c68acb074170a951033f2a`; the
  local release archive is
  `vowp-0.2.0-x86_64-unknown-linux-gnu.tar.gz`, SHA-256
  `71a99306d076cf274ba6b1469ab4ad8e654285ef235a39336c67b69f0b2fc28f`.
  The clean sudo run was repeated after the branch was rebased onto `e41459f` (the
  f-i1b verify fix); the rebased binary is
  `565400f10d549e5153d69e55b0bd87d104d10b4592eb8f02e9d093366afb2d74`, its archive
  `188c1b4ed3f9f0a63d58dbf6cd760b64464ace89e896e51cfe724ea14c99094f`. Among the
  setup sources, the plan/home/control/install sources are byte-identical between
  `a0d0aee` and `e41459f`; within them only `setup/verify.rs` and its tests changed.
  The full `a0d0aee..e41459f` diff also changes docs and `Cargo.toml`.
- Real-VM binary under test: a fresh `0.2.0` build of `a191c02`. The acceptance copy
  was `git archive HEAD` into a temporary tree with the workspace version and the
  inter-crate version pins changed from `0.1.0` to `0.2.0` (the J2 bump); the product
  code is unchanged. `cargo build --release -p vowp-cli --bin vowp` produced the
  binary SHA-256
  `2eb95aca2b16bef71d55ba081d71e74c57f6013daa2d72cf77ceed730c10ebf2`; the archive
  `vowp-0.2.0-x86_64-unknown-linux-gnu.tar.gz` has SHA-256
  `3be10075b9405e1b75373319355aa6f5b3e46004fe95a501bfe1cd2cc26fed4c`.
  The installed home binary has the same SHA-256. The archive was served from a
  loopback mirror because the public release is 404 (below).
- Real VM: `vowp-t1-fresh-1112`, zone `europe-north1-b`, machine type
  `e2-standard-2` (2 vCPU, 7.7 GiB), 20 GB boot disk, Ubuntu 24.04.5 LTS image
  `ubuntu-2404-noble-amd64-v20261105`; 15.9 GiB free before the run. It started with
  no `vowp` binary, no `vowp` user, no `/home/vowp`, no `~/.vowp` and no
  `~/.vowp-control`. The login user `dev` had passwordless sudo and a 0750 home; the
  `root` home was 0700. The orchestrator created the VM after the owner's yes on
  2026-11-11; this job did not create or delete it.
- Real-VM control machine: a temporary `$HOME`, a temporary SSH config and an `ssh`
  wrapper on the build VPS, with a dedicated acceptance key. The wrapper exists
  because OpenSSH reads the passwd home rather than `$HOME`; the real host `~/.ssh`
  config, `~/.vowp` and `~/.vowp-control` were not used.
- Real-VM test scaffolding: fake version-only `opencode` and `claude` binaries in
  `/usr/local/bin` and a dummy `opencode` auth marker, a generated `hello` fixture
  repository, a copy of `scripts/install.sh` and the 0.2.0 archive under
  `/home/acceptance-release`, and a loopback `python3 -m http.server` on
  `127.0.0.1:18080` behind a test-only `curl` shim. No real harness, model turn,
  provider key or public port was used; the shim only exists because the public tag
  is 404.
- Rig: the T0 images, `vowp-acceptance/vps:sudo` (`3c19867d4d63`),
  `vowp-acceptance/vps:no-sudo` (`335555ae638a`) and
  `vowp-acceptance/phone:local` (`91530d993554`). The home containers are Ubuntu
  24.04 with sshd, git, tmux, curl, sqlite3 and the rig fixtures. They start with no
  VOWP binary and no `~/.vowp`; this was checked before the plan.
- Private tmux: every Docker home had `TMUX_TMPDIR=/home/vowp/.acceptance/tmux` and a
  private socket (`vowp-acceptance-t1-*`); the Docker `setup verify` used the in-memory
  tmux fake. On the real VM the product supervisor started its own tmux server with a
  private `-L vowp-...` socket, which is the unit's normal behaviour. The host home,
  host `~/.vowp`, host `~/.vowp-control` and the default tmux server were never used.
- Control machines: a temporary `$HOME` on the host for the Docker `opencode` run, the
  rig's separate phone container for the `claude-code` run, and a second temporary
  `$HOME` on the build VPS for the real-VM run. All are "clean" (no
  `~/.vowp-control`) and reached the home over SSH with a dedicated key. The host
  controls used a temporary `ssh` wrapper only to point SSH at the test config, because
  OpenSSH reads the passwd home rather than `$HOME`; the real host `~/.ssh` was not
  touched.
- No real `claude`/`opencode` binary, model turn, provider key or public secret was
  used. The Docker homes and the real VM had fake harness binaries that only answer
  `--version`, plus fake login-marker files, so the setup preflight sees the same shape
  as a signed-in harness. The only product network traffic was the loopback release
  mirror described below; the only public traffic was read-only checks of the public
  release URL and tag and the GCE metadata service.

The commands below use these variables inside the home container:

```sh
V=/home/vowp/.local/bin/vowp
H=/home/vowp/.vowp
```

### What was simulated

1. **The public prompt and release.** On 2026-11-11 the same two checks returned
   **404** on the build VPS and on the GCE VM:

   ```text
   control agent-setup.md HTTP 404
   control release tag HTTP 404
   vm agent-setup.md HTTP 404
   vm release tag HTTP 404
   ```

   `https://raw.githubusercontent.com/syver777/vowp/v0.2.0/docs/setup/agent-setup.md`
   and the GitHub release `v0.2.0` do not exist. The runs therefore used
   `docs/setup/agent-setup.md` from the tested checkout (the file the tag will publish)
   and loopback release mirrors. The mirrors contain the archives above; `install.sh`
   performed its normal SHA-256 verification and printed `verified SHA256 71a99...`
   (Docker builds) or `verified SHA256 3be10075...` (real-VM build). A test-only
   `curl` shim served the repository's `install.sh` for the raw URL and pointed its
   default release base URL at `http://localhost:18082` on the control VPS and
   `http://localhost:18080` on the VM; every other URL went to the real `curl`.
   `install.sh` refuses non-local plain HTTP, so the mirrors stayed on loopback.
   Nothing was published.
2. **The two harnesses.** The card asks for the README prompt in opencode and Claude
   Code. This worker cannot run either real harness with a model under the repository
   rules (no real harness, network model turn or credential). The two harness
   identities were therefore the two distinct setup answers: `opencode` with provider
   `deepseek`, and `claude-code` with provider `anthropic`, each with a fake
   version-answering binary and a login marker in the home. The contract's commands
   were executed literally by a scripted driver. The real-VM setup answer was
   `opencode`; the fake `claude` binary was added only for the doctor cross-check. A
   real-harness click-through remains an owner/release check; it is not claimed here.
3. **The Google Cloud VM.** Created by the orchestrator after the owner's yes on
   2026-11-11 (e2-standard-2, europe-north1, Ubuntu 24.04, 20 GB, under $0.10/hour,
   same-day delete). `europe-north1-a` had no capacity, so the instance ran in
   `europe-north1-b`, in the same region. The run used the real image, sshd, sudo,
   systemd and network; only the harness identities, the public release URL and the
   fixture project were simulated as described above. This job was instructed not to
   delete the VM and did not delete it. The orchestrator deleted it with its boot disk
   on 2026-11-11 (delete operation at 13:12 UTC) and verified both
   `gcloud compute instances list` and `gcloud compute disks list` returned no items
   (see [Deletion](#deletion-orchestrator)).
4. **Real-VM fixture mistake.** The fixture that created the fake `opencode` sign-in
   used `install -d` with `-o vowp`, which left `/home/vowp/.local` and
   `.local/share` owned by root. The first install attempt after the workaround
   therefore failed with `mkdir: cannot create directory '/home/vowp/.local/bin':
   Permission denied`. `chown -R vowp:vowp /home/vowp/.local` fixed the fixture and the
   same product command then passed. This is test scaffolding, not a product defect;
   the final run below is the one recorded as the result.

## Test 1a — sudo rig with an existing dedicated user: PASS

Rig `t1-sudo2`: login as `vowp`, the image's dedicated user, with passwordless sudo
available. This is the case where the owner already created the unprivileged account
and the setup only has to use it. The home started with no VOWP and no `~/.vowp`.

The run answered the contract's home-target, harness, repository and control-machine
questions; the GitHub-remote answer was **no**. It ran the read-only plan twice (text
and `--json`):

```sh
$CONTROL/.local/bin/vowp setup plan --target t1-home2 --version 0.2.0 \
  --name t1-sudo2 --repository /home/vowp/projects/hello \
  --harness opencode --provider deepseek
$CONTROL/.local/bin/vowp setup plan --target t1-home2 --version 0.2.0 \
  --name t1-sudo2 --repository /home/vowp/projects/hello \
  --harness opencode --provider deepseek --json
```

Both exited **0**. The text plan printed the eight phases, their commands, files,
`requires` and idempotency, and only one hand-off (the approval); the optional GitHub
remote hand-off is absent because the answer was no. The `--json` document had exactly
the four documented top-level fields, and its `handoffs` array held only the
`preflight/approval` entry:

```text
sudo2-plan.json top-level: ['handoffs', 'phases', 'target', 'version']
```

The plan is read-only by construction; the home was still absent before the apply
(the image starts empty). The owner then approved the whole plan once, and the harness
ran:

```sh
$CONTROL/.local/bin/vowp setup home --target t1-home2 --version 0.2.0 \
  --name t1-sudo2 --repository /home/vowp/projects/hello \
  --harness opencode --provider deepseek --apply --yes
```

Exit **0**:

```text
setup: ok (phase tunnels)
changed:
  - /home/vowp/.local/bin/vowp
  - /home/vowp/.vowp/vowp.db
  - /home/vowp/.vowp
  - /home/vowp/.vowp/config.toml
  - /home/vowp/.vowp/vowp.toml
  - /home/vowp/.vowp/projects/_node/project.db
  - /home/vowp/.config/systemd/user/vowp.service
  - vault
  - /home/vowp/projects/hello
  - ~/.vowp-control/setup.json
checks:
  ok   preflight.ssh: Linux x86_64 as vowp (home /home/vowp)
  ok   preflight.tools: git, tmux and curl are present
  ok   preflight.disk: 71.2 GiB free on the home
  ok   user.mode: working as the dedicated vowp user
  ok   user.harness_login: the chosen harnesses are signed in
  ok   install.version: installed vowp 0.2.0
  ok   home.database: created the global database
  ok   home.doctor: vowp doctor reports no failures
  ok   service.unit: installed the user systemd unit
  warn service.enabled: systemd is not available; start the home with `vowp up --foreground`
  ok   vault.mode: vault user mode
  ok   projects: registered /home/vowp/projects/hello
  ok   tunnels: no tunnels requested
```

The only warning is the expected one in a container without systemd. The home's
profiles are the `opencode` ladder the answers asked for:

```text
[profiles.worker]
ladder = ["opencode:deepseek/deepseek-flash#max"]
```

The control phase then wrote the control home and found the installed binary:

```sh
$CONTROL/.local/bin/vowp setup control --apply
```

Exit **0**. It wrote `control.json`, `nodes.json`, `surfaces.json`, `tunnels.json` and
the private spool at mode 0600, reported `no supervisor is installed on a control
machine`, and warned that `snapshot.json` was empty until the home wrote one. The
verification then ran on the control machine:

```sh
$CONTROL/.local/bin/vowp setup verify
```

Exit **0**:

```text
setup: ok (phase verify)
changed: nothing
checks:
  ok   gc.dry_run: dry run lists 11 file(s) in 2 class(es); nothing was deleted
  ok   fixture.job_merge: fixture job setup-verify-job-... merged into main on home /home/vowp/.vowp (project setup-verify-job-...)
  ok   control.snapshot: snapshot as of 2026-11-11T10:22:00.472Z (home t1-sudo2, project(s) 1, source remote)
  ok   orchestrator.chat: orchestrator setup-verify-... answered through `vowp chat` in session 01M7521WA6J2DM7644M76311ZS on home /home/vowp/.vowp
```

The snapshot is readable from the control cache with an `as of` time, the fixture job
merged through the real merge queue, and the orchestrator answered through the chat
fallback.

### Idempotency, guard and JSON

- Re-running `setup home --apply --yes` exited **0**, reported `changed: nothing`,
  `install.version: vowp 0.2.0 is already installed`, `home.database: the global
  database is present`, `vault.mode: vault user mode` and
  `projects: /home/vowp/projects/hello is already registered`.
- Re-running `setup control --apply` exited **0**, reported `changed: nothing` and
  `snapshot cache as of 2026-11-11T10:22:00.472Z`.
- `setup home --apply` without `--yes` exited **5** and returned only the preflight
  `approval` hand-off (`changed: nothing`), before opening SSH.
- The idempotent `setup home --json` document had the five `SetupOutcome` fields
  (`changed`, `checks`, `handoff`, `phase`, `status`) and the `setup verify --json`
  document had the same five; the checks were `gc.dry_run`, `fixture.job_merge`,
  `control.snapshot` and `orchestrator.chat`, all `ok`.

### Rebased-tree re-run

`origin/main` advanced to `e41459f` while this card was in progress and brought the
f-i1b setup-verify fix. The branch was rebased onto it and the clean sudo path was
repeated from an empty rig with the rebased `0.2.0` build. `setup plan`,
`setup home --apply --yes`, `setup control --apply` and `setup verify` all exited
**0**; the install verified the rebased archive SHA-256 `188c1b4e...`, the verify
snapshot was as of `2026-11-11T11:02:09.574Z`, and the orchestrator chat session was
`01M754BCYJGY2E93X6K88WSCF2`. Among the setup sources, the plan/home/control/install
sources are identical between `a0d0aee` and `e41459f`; the repeat covers the changed
verify code (the full diff also changes docs and `Cargo.toml`).

## Test 1b — sudo rig, user-creation path: one product defect, then PASS

Rig `t1-sudo`: the login user was `owner` (passwordless sudo, home mode 0750), and the
container's pre-created `vowp` account was removed so the setup's user phase had to
create it. The home had a seeded harness login marker, and the seeded `/home/vowp` was
re-owned to the recreated UID after `useradd` (what `useradd --create-home` would do on
a fresh machine). This is the primary path the contract describes for a VPS where the
owner logs in as root or as their own sudo user.

The plan matched 1a's eight phases and used the same single approval, but it also
printed the `projects/owner-answer` GitHub-remote hand-off because this run's
GitHub-remote answer was yes (`sudo-plan.txt`). The first `setup home --apply --yes` ran
the preflight, created the `vowp` user and copied the login key, then failed at install:

```text
$ ... vowp setup home --target t1-home --version 0.2.0 ... --apply --yes
vowp: remote: installing vowp failed: sh: 24: cd: can't cd to .
exit 7
```

The cause is the remote command's inherited working directory. `setup` runs the
install command as the effective user:

```sh
sudo -u vowp -H env VOWP_HOME=/home/vowp/.vowp \
  sh -c 'curl -fsSL .../install.sh | sh -s -- --download --version 0.2.0 ...'
```

The SSH session starts in the login user's home (`/home/owner`), and the `sudo`
transition keeps that directory. `install.sh` computes its own directory with
`SCRIPT_DIR=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)` (`scripts/install.sh:22`);
with `$0` = `sh` this is `cd .`, and `vowp` cannot traverse a 0750 home. The same
failure occurs from `/root` (mode 0700), which is the common case when a fresh cloud
image is first reached as root. The reproduction on the rig was:

```text
$ ls -ld /home/owner
drwxr-x--- 4 owner owner 4096 ... /home/owner
$ sudo -u vowp sh -c "pwd; cd ."
/home/owner
sh: 1: cd: can't cd to .
```

A disposable `vps:sudo` container check from root confirmed the same failure without the
test user: `/root` is mode 0700, and `sudo -u vowp sh -c "cd ."` from `/root` exits 2
with `sh: 1: cd: can't cd to .`.

For the acceptance run the login home was made traversable (`chmod 0755 /home/owner`,
a normal home mode), and the same command was re-run. It then exited **0**; the
remaining checks were ok, including `install.version: installed vowp 0.2.0`,
`vault.mode: vault user mode` and `projects: registered /home/vowp/projects/hello`.
The control and `setup verify` also exited **0** with the same four `ok` checks. This
isolates the failure to the install-phase working directory, not to user creation,
the archive, the database or the control side.

**Open item 1 is the fix.** A robust fix is to give every remote setup command an
explicit working directory (for example `cd <effective_home> && exec ...` in the
`as_user`/`Executor::remote` path) or to make `install.sh` not `cd` when it is piped to
`sh`. The acceptance workaround is not a product fix.

## Test 1c — no-sudo rig with `claude-code` and a secret hand-off: PASS

Rig `t1-nosudo`: the SSH login was the unprivileged `vowp` user and the image has no
`sudo` binary at all. The control machine was the rig's separate phone container (its
own key, its own `/home/phone` control home), not the host. The contract answers were
`claude-code` + provider `anthropic` + secret name `ANTHROPIC_API_KEY`.

`setup plan` (text and `--json`) exited **0** and wrote nothing. It printed the no-sudo
user phase (`Use the no-sudo mode as the login user`), the basic vault mode, and the
secret hand-off. `setup home --apply --yes` exited **5** at the vault phase, exactly as
the contract requires:

```text
setup: handoff (phase vault)
handoff (vault/secret): Save ANTHROPIC_API_KEY with `vowp secret set ANTHROPIC_API_KEY`
  on the home (hidden prompt) or `vowp secret request ANTHROPIC_API_KEY` through a
  tunnel, then re-run setup. Never paste the value into the chat.
checks:
  ok   user.mode: working as the dedicated vowp user
  ok   vault.mode: basic mode (same user); sudo is not installed
  ok   service.unit: installed the user systemd unit
  warn service.enabled: systemd is not available; start the home with `vowp up --foreground`
```

The owner then entered a **dummy** value through the hidden prompt on the home. The
value is not printed here and was never a real credential; the product printed only
`ANTHROPIC_API_KEY saved, ends in …4f2a`, and `secret ls --json` returned the name,
`last4: "4f2a"` and timestamps. Because the automation had to drive a pty, the pty
echoed the piped line before the hidden prompt took it; that echo is an automation
artifact, not the product's prompt output. Re-running the same
`setup home --apply --yes` exited **0**:

```text
setup: ok (phase tunnels)
changed:
  - /home/vowp/projects/hello
checks:
  ok   install.version: vowp 0.2.0 is already installed
  ok   vault.mode: basic mode (same user); sudo is not installed
  ok   vault.secret: ANTHROPIC_API_KEY is saved
  ok   projects: registered /home/vowp/projects/hello
  warn service.enabled: systemd is not available; start the home with `vowp up --foreground`
```

`setup control --apply` and `setup verify` on the phone control both exited **0**; the
four verify checks were `ok`, with the snapshot as of `2026-11-11T10:20:33.360Z` and
the orchestrator chat session `01M751Z6WTP31PJ6EMKZP3Z8WW`. The home's profiles are the
`claude-code` ladder:

```text
[profiles.worker]
ladder = ["claude-code:claude-sonnet-5-5@medium"]
```

The secret store is basic mode (no `vowp-vault` user and no `secrets/mode` file in this
build); `secret ls` and the setup check both report the basic mode consistently. The
home opened no public port: `ss -tlnp` showed only sshd on `0.0.0.0:22` and the
test-only loopback release mirror on `127.0.0.1:18080`.

## Test 1d — real GCE VM (`opencode`, sudo, systemd): PASS after the known workaround

The orchestrator created `vowp-t1-fresh-1112` (zone `europe-north1-b`,
`e2-standard-2`, Ubuntu 24.04.5 LTS, 20 GB) after the owner's yes. The run started at
12:04 UTC from an empty image. Before the plan:

```text
date=2026-11-11T12:04:27Z
os=PRETTY_NAME="Ubuntu 24.04.5 LTS"
kernel=Linux 7.0.0-1017-gcp x86_64
cpu=2 mem=7GiB
disk=15.9 GiB free of 18.3 GiB
git=/usr/bin/git tmux=/usr/bin/tmux python3=/usr/bin/python3
opencode=/usr/local/bin/opencode
vowp=MISSING
login_user=dev uid=1001
login_home_mode=750
vowp_user=absent
vowp_home=absent
vault_home=absent
sudo=passwordless
```

`curl` is the test-only shim described above; `/usr/bin/curl` is the real one. The
control machine bootstrapped the pinned CLI from the control-side loopback mirror:

```text
install.sh: downloading http://localhost:18082/v0.2.0/vowp-0.2.0-x86_64-unknown-linux-gnu.tar.gz
install.sh: verified SHA256 3be10075b9405e1b75373319355aa6f5b3e46004fe95a501bfe1cd2cc26fed4c
install.sh: installed .../.local/bin/vowp
install.sh: installed: vowp 0.2.0
```

`vowp --version` printed `vowp 0.2.0`, and the binary's SHA-256 is the archive
binary's `2eb95aca...`. `setup plan` (text and `--json`) exited **0** and printed the
eight phases, their commands, files and requires, and two hand-offs (approval and
`user/login`). The plan's install command is the documented one:

```sh
sudo -u vowp -H env VOWP_HOME=/home/vowp/.vowp \
  sh -c 'curl -fsSL .../v0.2.0/scripts/install.sh | sh -s -- --download --version 0.2.0 ...'
```

### First apply: user created, harness-login hand-off

`setup home --apply --yes` exited **5**:

```text
setup: handoff (phase user)
changed:
  - user:vowp
  - ~/.vowp-control/setup.json
handoff (user/login): Sign in to opencode on the home as vowp with its own login flow, then re-run setup.
checks:
  ok   preflight.ssh: Linux x86_64 as dev (home /home/dev)
  ok   preflight.tools: git, tmux and curl are present
  ok   preflight.disk: 15.9 GiB free on the home
  ok   user.mode: created the vowp user and copied the SSH key
  fail user.harness_login: opencode is installed but not signed in for vowp
```

The fake `opencode` binary was already installed, but the fake sign-in marker was not.
A dummy `~/.local/share/opencode/auth.json` was created as `vowp` and the `hello`
fixture repository was generated under `/home/vowp/projects/hello`. A second
`setup plan` still listed the `user/login` hand-off even though the marker existed,
because the read-only probe runs as the login user `dev` with `HOME=/home/vowp`, and
`/home/vowp` is mode 0750. The apply's user phase runs the same check as `vowp` and
passes. This is a false positive in the plan, recorded as open item 7.

### Second apply: the install cwd defect on the real image

The same `setup home --apply --yes` then exited **7**:

```text
vowp: remote: installing vowp failed: sh: 22: cd: can't cd to .
```

This is the real-GCE reproduction of open item 1. The SSH session starts in the login
user's 0750 home; the `sudo -u vowp` transition keeps that directory; `install.sh`
computes its own directory with
`SCRIPT_DIR=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)`, which is `cd .` when the
script is piped to `sh`, and `vowp` cannot traverse `/home/dev`. The acceptance
workaround was the same one used in test 1b:

```text
$ sudo chmod 0755 /home/dev
login_home_mode=755
```

The first install attempt after that still failed with `mkdir: cannot create
directory '/home/vowp/.local/bin': Permission denied`, because the test fixture had
left `/home/vowp/.local` root-owned (see "What was simulated" item 4). After
`chown -R vowp:vowp /home/vowp/.local` the same product command completed.

### Third apply: home, service, vault and project: PASS

`setup home --apply --yes` exited **0**:

```text
setup: ok (phase tunnels)
changed:
  - /home/vowp/.local/bin/vowp
  - /home/vowp/.vowp/vowp.db
  - /home/vowp/.vowp
  - /home/vowp/.vowp/config.toml
  - /home/vowp/.vowp/vowp.toml
  - /home/vowp/.vowp/projects/_node/project.db
  - /home/vowp/.config/systemd/user/vowp.service
  - vault
  - /home/vowp/projects/hello
checks:
  ok   preflight.ssh: Linux x86_64 as dev (home /home/dev)
  ok   preflight.tools: git, tmux and curl are present
  ok   preflight.disk: 15.9 GiB free on the home
  ok   user.mode: the vowp user already exists
  ok   user.harness_login: the chosen harnesses are signed in
  ok   install.version: installed vowp 0.2.0
  ok   home.database: created the global database
  warn home.doctor: vowp doctor has warnings or failures: projects (warn): no projects registered; snapshot (warn): not written yet; the supervisor writes it every tick; harness:claude-code (fail): binary not found on PATH; providers (warn): DEEPSEEK_API_KEY: absent; systemd (warn): /home/vowp/.config/systemd/user/vowp.service; watchdog (warn): /home/vowp/.local/bin/vowp-watchdog.sh; supervisor (warn): not running
  ok   service.unit: installed the user systemd unit
  warn service.enabled: systemctl --user daemon-reload failed: Failed to connect to bus: No such file or directory
  ok   service.linger: linger is enabled
  ok   vault.mode: vault user mode
  ok   projects: registered /home/vowp/projects/hello
  ok   tunnels: no tunnels requested
```

The two warnings are the real-VM first-run gaps. The project and snapshot warnings are
expected at this point (the projects phase runs after the home doctor check, and the
supervisor has not started). The `harness:claude-code` failure is the optional harness
that was not part of the setup answer. `service.enabled` warns because the service
phase enables linger *after* its `systemctl --user` commands, so the user manager does
not exist yet. The idempotent re-run below starts the unit; open item 6 records the
ordering.

### Control, verify and the running home

`setup control --apply` on the control machine exited **0** and wrote the private
control files; its only warning was `control.snapshot: empty until the home writes
snapshot.json`. `setup verify` then exited **0**:

```text
setup: ok (phase verify)
checks:
  ok   gc.dry_run: dry run lists 11 file(s) in 2 class(es); nothing was deleted
  ok   fixture.job_merge: fixture job setup-verify-job-18e6fc2f10d7f160-0 merged into main on home /home/vowp/.vowp
  ok   control.snapshot: snapshot as of 2026-11-11T12:06:24.437Z (home vowp-t1-fresh, project(s) 1, source remote)
  ok   orchestrator.chat: orchestrator setup-verify-18e6fc2f7b0730db-2 answered through `vowp chat` in session 01M75810YFJBSX2S12BFXZZS6N
```

`setup verify --json` repeated the same four `ok` checks (snapshot
`2026-11-11T12:06:31.892Z`, chat session `01M75817B9MHTY5E7D7YGPAH82`). The second run
listed 20 files in 4 gc classes because the first run's fixture artifacts now existed;
nothing was deleted.

The idempotent `setup home --apply --yes` exited **0** and reported
`changed: systemd:vowp.service`, `install.version: vowp 0.2.0 is already installed`,
`projects: /home/vowp/projects/hello is already registered`, and:

```text
ok   service.enabled: enabled and started vowp.service
ok   service.linger: linger is enabled
```

The idempotent `setup control --apply` exited **0** with
`control.snapshot: snapshot cache as of 2026-11-11T12:07:01.459Z`. A final
`setup home --apply --yes --json` exited **0** with all fourteen checks `ok`
(`preflight.*`, `user.*`, `install.version`, `home.database`, `home.doctor`,
`service.*`, `vault.mode`, `projects`, `tunnels`); `setup control --apply --json`
exited **0** with the snapshot cache as of `2026-11-11T12:10:06.744Z`. The guard
without `--yes` exited **5** with only the approval hand-off and `changed: nothing`.

The real systemd user manager was running:

```text
linger=yes
enabled
active
Main PID: 7940 (vowp)
  /home/vowp/.local/bin/vowp up --foreground
  tmux -L vowp-a5312a872f844163 new-session -d -P -F #{window_id} -s vowp-home -n scratch -- ...
```

`journalctl --user -u vowp.service` showed `Started vowp.service` and
`supervisor running (pid 7940); stop it with 'vowp down'`. The tmux server is the
product's own private `-L vowp-...` socket.

### Home, doctor, phone and network

The installed home binary reports `vowp 0.2.0` and has SHA-256 `2eb95aca...`, the same
as the release archive. From the unit's working directory (`/home/vowp/.vowp`) with
both fake harnesses installed, `vowp doctor --json` exited **0** and reported every
check `ok` except the expected warnings `providers: DEEPSEEK_API_KEY: absent` and
`watchdog: ...vowp-watchdog.sh`; the supervisor check was
`running (pid 7940)`. From the login user's cwd `/home/dev`, the same doctor command
exited **1** because `harness:claude-code` is not installed and the config/schedules/
profiles/providers checks cannot find a `vowp.toml`; the setup's own `home.doctor`
check treats those as warnings. The final setup JSON run (after the fake `claude` was
added) reports `home.doctor: vowp doctor reports no failures`.

The home layout is the documented one:

```text
/home/vowp/.vowp/vowp.db                 global database
/home/vowp/.vowp/config.toml             node config
/home/vowp/.vowp/vowp.toml               node project config
/home/vowp/.vowp/projects/_node/project.db
/home/vowp/.vowp/projects/hello/project.db
/home/vowp/.vowp/snapshot.json            supervisor snapshot
/home/vowp/.vowp/heartbeat                supervisor heartbeat
/home/vowp/.vowp/secrets/mode             "vault"
/home/vowp-vault/                         vault home (root-owned)
/home/vowp/.config/systemd/user/vowp.service
```

`project ls --json` lists `hello` as `active` at `/home/vowp/projects/hello` with the
local bare `origin`; `fleet status --json` shows the supervisor running (pid 7940),
one project, capacity target 1 (auto), and the capacity decision inbox item created by
the running supervisor. The vault user `vowp-vault` exists with the helper and
sudoers file, and `secrets/mode` is `vault`. The phone path works with key-only SSH as
the dedicated user:

```text
$ ssh vowp@<vm> 'bash -lc "command -v vowp; hostname; echo HOME=$HOME; test -f $HOME/.vowp/vowp.db && echo vowp.db=present"'
/home/vowp/.local/bin/vowp
HOME=/home/vowp
vowp.db=present
```

`ss -tlnp` on the VM shows only sshd on `0.0.0.0:22` and `[::]:22`, the loopback test
mirror on `127.0.0.1:18080`, and systemd-resolved on loopback. No product port is
public. The control cache contains `control.json`, `nodes.json`, `surfaces.json`,
`tunnels.json`, `snapshot.json` and `snapshot.meta.json`, all mode 0600; the snapshot
was fetched with `source: remote` and the meta says `fetched_at:
2026-11-11T12:07:01.459Z`.

### Control-routed fleet commands: new defect

On the control machine, the read-only fleet commands route to the home with a bare
`vowp`:

```text
$ vowp fleet status --json
bash: line 1: vowp: command not found
exit 127
$ vowp fleet queue --json
bash: line 1: vowp: command not found
exit 127
$ vowp fleet inbox --json
bash: line 1: vowp: command not found
exit 127
$ vowp fleet status --cached --json
{ "snapshot": { ... "home": "vowp-t1-fresh" ... }, "source": "cache", ... }
exit 0
```

The installed home binary is `/home/vowp/.local/bin/vowp`; a non-login SSH command does
not source `~/.profile`, so `vowp` is not on its PATH. The setup commands avoid this by
using the absolute path. The cache fallback only runs on an ssh failure (255), not on
the remote 127, so `vowp fleet status` fails instead of printing the cached snapshot.
`fleet status --cached` works. This is open item 5; it does not affect the setup or
verify checks, which use the absolute path.

### Deletion (orchestrator)

The orchestrator deleted the VM and its boot disk on 2026-11-11 (delete operation at
13:12 UTC). Both `gcloud compute instances list` and `gcloud compute disks list`
returned no items. This job did not delete it; these are the orchestrator's commands:

```sh
gcloud compute instances delete vowp-t1-fresh-1112 \
  --zone=europe-north1-b --delete-disks=boot --quiet
gcloud compute instances list --filter='name=vowp-t1-fresh-1112'
gcloud compute disks list --filter='name=vowp-t1-fresh-1112'
```

## Cross-checks

- **Phone reaches the home.** After 1a, `rig.sh phone` SSHed in with the phone key and
  printed `vowp`, `vps`, `/home/vowp/.vowp` and `/home/vowp/.vowp/vowp.db`. On the real
  VM, key-only SSH as `vowp` printed `/home/vowp/.local/bin/vowp`, the home path and
  `vowp.db=present`. This is the documented "phone needs only an SSH app and the key"
  path; the full app surface is T3.
- **Home layout.** Both Docker homes and the real VM contain `vowp.db`, `config.toml`,
  `vowp.toml`, `projects/_node/project.db`, the registered `hello` project database and
  `snapshot.json`; the global DB lists the node and the `hello` project as `active`.
  The real VM also has the vault mode file, the vault home, the systemd unit and a
  heartbeat written by the running supervisor.
- **Doctor.** On the Docker homes, `vowp doctor --json` reports `ok` for the home,
  database integrity/schema, projects, snapshot, git, tmux, harnesses, systemd unit and
  disk. It warns that the optional watchdog is not installed (`setup` installs with
  `--no-watchdog`), that the supervisor is not running in the container, and that no
  `vowp.toml` is found in the login user's current directory. On the real VM, the
  doctor run from the unit's working directory (`/home/vowp/.vowp`) with both fake
  harnesses installed exited **0** with only the provider-key and watchdog warnings;
  the supervisor check was `running (pid 7940)`. The same doctor from `/home/dev`
  exits **1** on `harness:claude-code` (not installed) and the missing-cwd config
  checks; the setup's `home.doctor` check reports those as warnings. None blocked
  setup.
- **No host data.** The runs used temporary `VOWP_HOME`s inside containers and on the
  real VM, temporary control homes, dedicated generated keys and private tmux
  directories. No host `~/.vowp`, host `~/.vowp-control`, host SSH config/key or default
  tmux server was used.

## Open items

1. **Fix job: `setup home` install fails when the login home is not traversable by the
   target user.** Exact failure and cause are in test 1b; test 1d reproduced it on the
   real GCE image with `sh: 22: cd: can't cd to .`. Suggested owner paths are
   `crates/vowp-cli/src/setup/commands.rs` (run remote commands in the effective home,
   or wrap the install command with `cd <effective_home>`), and/or
   `scripts/install.sh` (do not `cd` when `$0` is `sh`). This is a release blocker for
   the common root-login and 0750-home cases; both the Docker and the VM runs only
   continued after `chmod 0755` on the test login home. A follow-up fix job owns this.
2. **Release gate: the literal README prompt is not followable until J2.** On
   2026-11-11 the prompt's `raw.githubusercontent.com/.../v0.2.0/...` and the
   `v0.2.0` release both return 404 from the control VPS and from the VM. The contract
   path and the install path were exercised with the tested file and a local archive,
   but a post-release re-run must fetch the real tag and pass the real checksum. This
   is a plan ordering issue (T1 is Wave 7, J2 is Wave 8), not a code failure found
   here.
3. **Owner/real-harness check: real opencode and Claude Code.** The Docker run and the
   real-VM run used fake harness binaries and login markers because the repository
   rules forbid real harness processes, model turns, network credentials and the real
   `~/.claude` / opencode stores in tests. The two harness identities were exercised as
   distinct setup answers and profiles; the real-VM setup answer was `opencode`. A
   release-time owner check should paste the README prompt into a real opencode and a
   real Claude Code on a clean control machine; this is the only way to test the
   harness's own plan/ask/approval behaviour.
4. **Observation, not a blocker: doctor warnings after setup.** `vowp doctor` warns
   that the watchdog is absent (`setup` installs with `--no-watchdog`) and, from a cwd
   outside the home, that no `vowp.toml` is found even though the home has
   `~/.vowp/vowp.toml`. It also fails `harness:claude-code` when only the chosen
   `opencode` is installed; the setup's `home.doctor` check reports that as a warning.
   From the unit's working directory (`/home/vowp/.vowp`) with both fake harnesses, the
   real-VM doctor exited **0** with only the provider-key and watchdog warnings. A
   follow-up may want an unconfigured optional harness to be a warning, the doctor to
   find the node config without relying on cwd, and the docs to state the watchdog and
   provider warnings are expected.
5. **Control-machine defect: fleet commands call a bare `vowp` on the home.** On the
   real-VM control, `vowp fleet status --json`, `fleet queue --json` and
   `fleet inbox --json` all exited **127** with `bash: line 1: vowp: command not
   found`, because the installed binary is `/home/vowp/.local/bin/vowp` and a
   non-login SSH PATH does not include `~/.local/bin`. `vowp fleet status --cached
   --json` exits **0** and prints the cached snapshot. The cache fallback only runs on
   an ssh failure (255), not on the remote 127. Suggested owner paths are
   `crates/vowp-cli/src/remote.rs` (use the installed absolute path or a login shell
   for the routed command, and treat any remote failure as a cache fallback) and the
   control-machine docs/tests. This does not affect `setup home`, `setup control` or
   `setup verify`, which use the absolute path.
6. **First-run service warning on a real VM: linger is enabled after
   `systemctl --user`.** In the first successful `setup home` on the fresh VM,
   `service.enabled` warned `systemctl --user daemon-reload failed: Failed to connect
   to bus: No such file or directory`; `service.linger` then succeeded. The idempotent
   re-run reported `service.enabled: enabled and started vowp.service`, and the unit
   ran as pid 7940. Suggested fix: run `loginctl enable-linger` before the
   `systemctl --user` commands (or retry them once after linger). Not a blocker for
   the final state, but the first apply should start the service without a re-run.
7. **Plan false positive: harness-login probe runs as the login user.** With the fake
   `opencode` sign-in in place, `setup plan` still printed the `user/login` hand-off
   because the probe runs as `dev` with `HOME=/home/vowp` and `/home/vowp` is 0750;
   the apply's user phase checks as `vowp` and reports
   `user.harness_login: the chosen harnesses are signed in`. Suggested fix: run the
   probe's harness-login check as the effective user (through the same sudo transition
   the apply uses) when the effective user differs. This is cosmetic for the apply but
   makes the plan misleading on the default GCE home mode.

The orchestrator has since deleted `vowp-t1-fresh-1112` and its boot disk (delete
operation at 13:12 UTC on 2026-11-11); both `gcloud compute instances list` and
`gcloud compute disks list` returned no items. This job did not delete it (see
[Deletion](#deletion-orchestrator)).

## How to reproduce

On a fresh `vps:sudo` rig (`bash scripts/acceptance/rig.sh up --name <name> --variant
sudo`; `--variant` belongs to `rig.sh up`, not to `vowp setup`), use the image's `vowp`
user (or create a login user, set its home to 0755, and remove the image's `vowp`
account to exercise creation), seed fake harness binaries and login markers, and prepare
the local release mirror:

```sh
# Inside the home container, as root:
mkdir -p /usr/local/share/vowp-acceptance /home/acceptance-release
# install.sh from the tested checkout:
cp /path/to/acceptance/install.sh /usr/local/share/vowp-acceptance/install.sh
# the local 0.2.0 archive and its .sha256:
cp -R /path/to/release/v0.2.0 /home/acceptance-release/
python3 -m http.server 18080 --bind 127.0.0.1 --directory /home/acceptance-release &
```

The `curl` shim serves the tested `install.sh` for the raw URL with its default
`RELEASE_BASE_URL` pointed at `http://localhost:18080`; all other URLs go to
`/usr/bin/curl`. Then on a clean control home with a private SSH config/alias for the
rig port, use the installed absolute path (`~/.local/bin` is not on a non-login SSH
PATH):

```sh
V=$HOME/.local/bin/vowp
"$V" setup plan --target <alias> --version 0.2.0 --name <home> \
  --repository /home/vowp/projects/hello --harness opencode --provider deepseek
"$V" setup home --target <alias> --version 0.2.0 --name <home> \
  --repository /home/vowp/projects/hello --harness opencode --provider deepseek \
  --apply --yes
"$V" setup control --apply
"$V" setup verify
```

### Real GCE VM (the recorded run)

The orchestrator creates the VM only after the owner's yes, then gives the job the
target. The recorded run used:

```sh
gcloud compute instances create <vm> \
  --zone=europe-north1-b \
  --machine-type=e2-standard-2 \
  --image-family=ubuntu-2404-lts-amd64 \
  --image-project=ubuntu-os-cloud \
  --boot-disk-size=20GB --boot-disk-type=pd-balanced \
  --metadata=ssh-keys="<owner-public-key>"
```

The product commands are the same; the VM-specific parts are the fake harness, the
loopback archive mirror and the 0750-home workaround. Copy `scripts/install.sh`,
`scripts/acceptance/` and the 0.2.0 archive plus `.sha256` from the tested checkout to
the VM first:

```sh
scp scripts/install.sh <vm>:/tmp/vowp-install.sh
scp -r scripts/acceptance <vm>:/tmp/vowp-acceptance-scripts
scp release/v0.2.0/vowp-0.2.0-x86_64-unknown-linux-gnu.tar.gz* <vm>:/tmp/
```

On the VM, as the login user with passwordless sudo:

```sh
# A fake harness: tests never use a real harness or credential.
sudo tee /usr/local/bin/opencode >/dev/null <<'SH'
#!/bin/sh
case "${1:-}" in
  --version|-v|version) printf '%s\n' '1.0.0' ;;
  *) printf '%s\n' 'opencode (acceptance fake)' ;;
esac
exit 0
SH
sudo chmod 0755 /usr/local/bin/opencode

# The release mirror: the tested install.sh, the fixtures and the archive.
sudo mkdir -p /usr/local/share/vowp-acceptance /home/acceptance-release/v0.2.0
sudo cp /tmp/vowp-install.sh /usr/local/share/vowp-acceptance/install.sh
sudo cp -a /tmp/vowp-acceptance-scripts/. /usr/local/share/vowp-acceptance/scripts/
sudo cp /tmp/vowp-0.2.0-x86_64-unknown-linux-gnu.tar.gz* /home/acceptance-release/v0.2.0/
sudo python3 -m http.server 18080 --bind 127.0.0.1 \
  --directory /home/acceptance-release >/var/log/vowp-acceptance-mirror.log 2>&1 &
sudo tee /usr/local/bin/curl >/dev/null <<'SH'
#!/bin/sh
case "$*" in
  *raw.githubusercontent.com/syver777/vowp/v0.2.0/scripts/install.sh*)
    sed 's|^RELEASE_BASE_URL=${VOWP_RELEASE_BASE_URL:-$DEFAULT_RELEASE_BASE_URL}|RELEASE_BASE_URL=${VOWP_RELEASE_BASE_URL:-http://localhost:18080}|' \
      /usr/local/share/vowp-acceptance/install.sh
    exit 0
    ;;
esac
exec /usr/bin/curl "$@"
SH
sudo chmod 0755 /usr/local/bin/curl
```

The control side is the same recipe with a temporary `$HOME`, a private SSH config/alias
and the same `curl` shim pointed at its own loopback mirror (port 18082 in the recorded
run), plus an `ssh` wrapper that adds `-F <ssh_config>` and a private
`UserKnownHostsFile`. Run the four setup commands from the Docker recipe. The first
`setup home` creates `vowp` and stops at the fake sign-in; seed the marker and the
`hello` fixture as `vowp`, then re-run:

```sh
sudo -u vowp -H mkdir -p /home/vowp/.local/share/opencode
printf '%s\n' '{}' | sudo -u vowp -H tee /home/vowp/.local/share/opencode/auth.json >/dev/null
sudo -u vowp -H bash /usr/local/share/vowp-acceptance/scripts/fixtures/project.sh \
  hello /home/vowp/projects/hello
```

The second run reproduces `sh: 22: cd: can't cd to .`; `sudo chmod 0755 /home/dev`
and a third run complete. The idempotent re-run starts the systemd unit. For the
recorded run the orchestrator deleted the VM and its boot disk after the report and
verified both lists were empty:

```sh
gcloud compute instances delete <vm> --zone=europe-north1-b --delete-disks=boot --quiet
gcloud compute instances list --filter="name=<vm>"
gcloud compute disks list --filter="name=<vm>"
```

Both lists must be empty. Never leave a monthly plan or an undeleted boot disk.

For the no-sudo variant, start the rig with `bash scripts/acceptance/rig.sh up
--name <name> --variant no-sudo` (`--variant` is a `rig.sh up` flag, not a `vowp setup`
flag), log in as `vowp`, replace the harness/provider answers with
`claude-code`/`anthropic`, and add `--secret ANTHROPIC_API_KEY`; expect exit 5 at the
vault hand-off, enter a dummy value through the hidden prompt, and re-run the same
command. The rig's `fixtures/project.sh` creates the `hello` checkout and its local
origin. Delete the rigs with `bash scripts/acceptance/rig.sh down --name <name>`.

## Gate status

This card changes only this document. The workspace gates run on the final commit are
reported in the job report; `cargo fmt --all`,
`cargo clippy --workspace --all-targets -- -D warnings` and
`cargo test --workspace` are run on the same tree.
