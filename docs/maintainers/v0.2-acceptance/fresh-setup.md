# v0.2 acceptance — fresh setup (T1)

This is the acceptance record for §13.5 test **1 (a fresh Linux machine set up from zero
by the README prompt, first on the Docker rig and then on one Google Cloud VM)**. It is
the T1 card `v2-t1-fresh-setup`.

**Result on the tested build: the Docker half of test 1 passes on both rig variants.**
A sudo home with a dedicated `vowp` user already present, and a no-sudo home driven by
the `claude-code` answer, both go from an empty container to a working home, control
machine and `setup verify` with no product failure. The sudo create-user path also
passes, but only after a one-line workaround for a product defect: `setup home` fails
its install phase when the SSH login user's home is not traversable by the target user
(open item 1). The real **Google Cloud VM half was not run**: the brief's Google Cloud
gate forbids creating a VM without the owner's explicit yes, so the exact request is in
open item 3. The literal README prompt URL returns **404** on 2026-11-11 because the
`v0.2.0` tag and release are not published yet (J2 is later); the run used the same
contract file from the tested checkout and a local release mirror, described under
[What was simulated](#what-was-simulated).

## Environment

- Date: 2026-11-11 (UTC). The main run finished at 10:22 UTC; the rebased re-run
  finished at 11:02 UTC.
- Worktree: `job/v2-t1-fresh-setup`, based on `a0d0aee` (`origin/main` at the time)
  and later rebased onto `e41459f` for the setup-verify fix.
- Binary under test: built from the HEAD source with
  `cargo build -p vowp-cli --bin vowp`. The workspace package version is still
  `0.1.0`; because the release tag does not exist, the acceptance copy used for the
  install was built from `git archive HEAD` with only the workspace version changed to
  `0.2.0` (the J2 version bump). The product code is identical. Its SHA-256 is
  `f610364b327ece5e790145e0ba42b3918e290500e2c68acb074170a951033f2a`; the
  local release archive is
  `vowp-0.2.0-x86_64-unknown-linux-gnu.tar.gz`, SHA-256
  `71a99306d076cf274ba6b1469ab4ad8e654285ef235a39336c67b69f0b2fc28f`.
  The clean sudo run was repeated after the branch was rebased onto `e41459f` (the
  f-i1b verify fix); the rebased binary is
  `565400f10d549e5153d69e55b0bd87d104d10b4592eb8f02e9d093366afb2d74`, its archive
  `188c1b4ed3f9f0a63d58dbf6cd760b64464ace89e896e51cfe724ea14c99094f`. The
  plan/home/control/install sources are byte-identical between `a0d0aee` and
  `e41459f`; only `setup/verify.rs` and its tests changed.
- Rig: the T0 images, `vowp-acceptance/vps:sudo` (`3c19867d4d63`),
  `vowp-acceptance/vps:no-sudo` (`335555ae638a`) and
  `vowp-acceptance/phone:local` (`91530d993554`). The home containers are Ubuntu
  24.04 with sshd, git, tmux, curl, sqlite3 and the rig fixtures. They start with no
  VOWP binary and no `~/.vowp`; this was checked before the plan.
- Private tmux: every home had `TMUX_TMPDIR=/home/vowp/.acceptance/tmux` and a private
  socket (`vowp-acceptance-t1-*`). `setup verify` used the in-memory tmux fake; no real
  tmux server was started. The host home, host `~/.vowp`, host `~/.vowp-control` and
  the default tmux server were never used.
- Control machines: a temporary `$HOME` on the host for the `opencode` run, and the
  rig's separate phone container for the `claude-code` run. Both are "clean" (no
  `~/.vowp-control`) and both reached the home over SSH with a generated key. The host
  control used a temporary `ssh` wrapper only to point SSH at the test config, because
  OpenSSH reads the passwd home rather than `$HOME`; the real host `~/.ssh` was not
  touched.
- No real `claude`/`opencode` binary, model turn, provider key or public secret was
  used. The home had fake harness binaries that only answer `--version`, plus fake
  login-marker files, so the setup preflight sees the same shape as a signed-in
  harness. The only network traffic was the loopback release mirror described below
  and read-only checks of the public release URL and tag.

The commands below use these variables inside the home container:

```sh
V=/home/vowp/.local/bin/vowp
H=/home/vowp/.vowp
```

### What was simulated

1. **The public prompt and release.** On 2026-11-11:

   ```text
   agent-setup.md HTTP 404
   release tag HTTP 404
   ```

   `https://raw.githubusercontent.com/syver777/vowp/v0.2.0/docs/setup/agent-setup.md`
   and the GitHub release `v0.2.0` do not exist. The run therefore used
   `docs/setup/agent-setup.md` from the tested checkout (the file the tag will
   publish) and a loopback release mirror. The mirror contains the archive above;
   `install.sh` performed its normal SHA-256 verification and printed
   `verified SHA256 71a99...`. A test-only `curl` shim served the repository's
   `install.sh` for the raw URL and exported
   `VOWP_RELEASE_BASE_URL=http://localhost:18080`; every other URL went to the real
   `curl`. `install.sh` refuses non-local plain HTTP, so the mirror stayed inside the
   container. Nothing was published.
2. **The two harnesses.** The card asks for the README prompt in opencode and Claude
   Code. This worker cannot run either real harness with a model under the repository
   rules (no real harness, network model turn or credential). The two harness
   identities were therefore the two distinct setup answers: `opencode` with provider
   `deepseek`, and `claude-code` with provider `anthropic`, each with a fake
   version-answering binary and a login marker in the home. The contract's commands
   were executed literally by a scripted driver. A real-harness click-through is an
   owner/release check (open item 4); it is not claimed here.
3. **The Google Cloud VM.** Not created. See open item 3 for the request.

## Test 1a — sudo rig with an existing dedicated user: PASS

Rig `t1-sudo2`: login as `vowp`, the image's dedicated user, with passwordless sudo
available. This is the case where the owner already created the unprivileged account
and the setup only has to use it. The home started with no VOWP and no `~/.vowp`.

The run used the contract's five answers listed here (home target, harness, repositories,
GitHub remote, control machine) and ran the read-only plan twice (text and `--json`):

```sh
$CONTROL/.local/bin/vowp setup plan --target t1-home2 --version 0.2.0 \
  --name t1-sudo2 --repository /home/vowp/projects/hello \
  --harness opencode --provider deepseek
$CONTROL/.local/bin/vowp setup plan --target t1-home2 --version 0.2.0 \
  --name t1-sudo2 --repository /home/vowp/projects/hello \
  --harness opencode --provider deepseek --json
```

Both exited **0**. The text plan printed the eight phases, their commands, files,
`requires` and idempotency, and two hand-offs (the approval and the optional GitHub
remote). The `--json` document had exactly the four documented top-level fields:

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
`01M754BCYJGY2E93X6K88WSCF2`. The plan/home/control/install sources are identical
between `a0d0aee` and `e41459f`; the repeat covers the changed verify code.

## Test 1b — sudo rig, user-creation path: one product defect, then PASS

Rig `t1-sudo`: the login user was `owner` (passwordless sudo, home mode 0750), and the
container's pre-created `vowp` account was removed so the setup's user phase had to
create it. The home had a seeded harness login marker, and the seeded `/home/vowp` was
re-owned to the recreated UID after `useradd` (what `useradd --create-home` would do on
a fresh machine). This is the primary path the contract describes for a VPS where the
owner logs in as root or as their own sudo user.

The plan and approval were the same as 1a. The first `setup home --apply --yes` ran the
preflight, created the `vowp` user and copied the login key, then failed at install:

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
`SCRIPT_DIR=$(cd -- "$(dirname -- "$0")" && pwd)`; with `$0` = `sh` this is `cd .`, and
`vowp` cannot traverse a 0750 home. The same failure occurs from `/root` (mode 0700),
which is the common case when a fresh cloud image is first reached as root. The
reproduction on the rig was:

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

## Cross-checks

- **Phone reaches the home.** After 1a, `rig.sh phone` SSHed in with the phone key and
  printed `vowp`, `vps`, `/home/vowp/.vowp` and `/home/vowp/.vowp/vowp.db`. This is the
  documented "phone needs only an SSH app and the key" path; the full app surface is
  T3.
- **Home layout.** Both homes contain `vowp.db`, `config.toml`, `vowp.toml`,
  `projects/_node/project.db`, the registered `hello` project database and
  `snapshot.json`; the global DB lists the node and the `hello` project as `active`.
- **Doctor.** On both homes, `vowp doctor --json` reports `ok` for the home, database
  integrity/schema, projects, snapshot, git, tmux, harnesses, systemd unit and disk.
  It warns that the optional watchdog is not installed (`setup` installs with
  `--no-watchdog`), that the supervisor is not running in the container, and that no
  `vowp.toml` is found in the login user's current directory. Those are warnings, not
  failures, and none blocked setup.
- **No host data.** The run used temporary `VOWP_HOME`s inside containers, temporary
  control homes, generated keys and the rig's private tmux directories. No host
  `~/.vowp`, host `~/.vowp-control`, host SSH key or default tmux server was used.

## Open items

1. **Fix job: `setup home` install fails when the login home is not traversable by the
   target user.** Exact failure and cause are in test 1b. Suggested owner paths are
   `crates/vowp-cli/src/setup/commands.rs` (run remote commands in the effective home,
   or wrap the install command with `cd <effective_home>`), and/or
   `scripts/install.sh` (do not `cd` when `$0` is `sh`). This is a release blocker for
   the common root-login and 0750-home cases; the Docker run only continued after
   `chmod 0755` on the test login home.
2. **Release gate: the literal README prompt is not followable until J2.** On
   2026-11-11 the prompt's `raw.githubusercontent.com/.../v0.2.0/...` and the
   `v0.2.0` release both return 404. The contract path and the install path were
   exercised with the tested file and a local archive, but a post-release re-run must
   fetch the real tag and pass the real checksum. This is a plan ordering issue (T1 is
   Wave 7, J2 is Wave 8), not a code failure found here.
3. **Owner gate: the one GCE VM run is not started.** Request for the orchestrator:
   approve **one** on-demand `e2-standard-2` VM (2 vCPU, 8 GiB), Ubuntu 24.04 LTS,
   `europe-north1-a` (or `us-central1-a` if the owner prefers), 20 GB `pd-balanced`
   boot disk. Approximate list price: about **$0.08/hour** in `europe-north1`
   (about $0.07/hour in `us-central1`) including the disk, well under the plan's
   $0.10/hour; a 1–2 hour run is under **$0.20**. No monthly plan. Create only after
   the owner's explicit yes, use key-only SSH, run the README prompt, then delete the
   instance **and its boot disk the same day**:

   ```sh
   gcloud compute instances create "$VM" \
     --zone=europe-north1-a \
     --machine-type=e2-standard-2 \
     --image-family=ubuntu-2404-lts-amd64 \
     --image-project=ubuntu-os-cloud \
     --boot-disk-size=20GB --boot-disk-type=pd-balanced \
     --metadata=ssh-keys="<owner-public-key>"
   # ... run the acceptance, then:
   gcloud compute instances delete "$VM" --zone=europe-north1-a \
     --delete-disks=boot --quiet
   gcloud compute instances list --filter="name=$VM"
   gcloud compute disks list --filter="name=$VM"
   ```

   Both lists must be empty after the deletion. The VM run would repeat 1a/1c with the
   real public prompt and a real harness login; it is the remaining evidence for the
   card.
4. **Owner/real-harness check: real opencode and Claude Code.** The Docker run used
   fake harness binaries and login markers because the repository rules forbid real
   harness processes, model turns, network credentials and the real `~/.claude` /
   opencode stores in tests. The two harness identities were exercised as distinct
   setup answers and profiles. A release-time owner check should paste the README
   prompt into a real opencode and a real Claude Code on a clean control machine; this
   is the only way to test the harness's own plan/ask/approval behaviour.
5. **Observation, not a blocker: doctor warnings after setup.** `vowp doctor` warns
   that the watchdog is absent (`setup` installs with `--no-watchdog`), that the
   supervisor is not running in the container, and that no project `vowp.toml` is
   found in the login user's current directory even though the home has
   `~/.vowp/vowp.toml`. The setup outcome treats these as warnings, which is why
   `home.doctor` is `ok`. A follow-up may want the setup-installed home to produce a
   clean doctor report, or the docs to state these warnings are expected.

## How to reproduce

On a fresh `vps:sudo` rig, use the image's `vowp` user (or create a login user, set its
home to 0755, and remove the image's `vowp` account to exercise creation), seed fake
harness binaries and login markers, and prepare the local release mirror:

```sh
# Inside the home container, as root:
mkdir -p /usr/local/share/vowp-acceptance /home/acceptance-release
# install.sh from the tested checkout:
cp /path/to/acceptance/install.sh /usr/local/share/vowp-acceptance/install.sh
# the local 0.2.0 archive and its .sha256:
cp -R /path/to/release/v0.2.0 /home/acceptance-release/
python3 -m http.server 18080 --bind 127.0.0.1 --directory /home/acceptance-release &
```

The `curl` shim serves `install.sh` for the raw URL and prepends
`VOWP_RELEASE_BASE_URL=http://localhost:18080`; all other URLs go to `/usr/bin/curl`.
Then on a clean control home with a private SSH config/alias for the rig port:

```sh
vowp setup plan --target <alias> --version 0.2.0 --name <home> \
  --repository /home/vowp/projects/hello --harness opencode --provider deepseek
vowp setup home --target <alias> --version 0.2.0 --name <home> \
  --repository /home/vowp/projects/hello --harness opencode --provider deepseek \
  --apply --yes
vowp setup control --apply
vowp setup verify
```

For the no-sudo variant, use `--variant no-sudo`, log in as `vowp`, replace the
harness/provider answers with `claude-code`/`anthropic`, and add
`--secret ANTHROPIC_API_KEY`; expect exit 5 at the vault hand-off, enter a dummy value
through the hidden prompt, and re-run the same command. The rig's `fixtures/project.sh`
creates the `hello` checkout and its local origin. Delete the rigs with
`bash scripts/acceptance/rig.sh down --name <name>`.

## Gate status

This card changes only this document. The workspace gates run on the final commit are
reported in the job report; `cargo fmt --all`,
`cargo clippy --workspace --all-targets -- -D warnings` and
`cargo test --workspace` are run on the same tree.
