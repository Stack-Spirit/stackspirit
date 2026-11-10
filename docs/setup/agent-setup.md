# Agent setup contract — VOWP v0.2 (draft)

> **Status: planned (draft, v0.2).** This is a draft of the contract that the README prompt points
> every harness to. `vowp setup` does not exist in v0.1; the `vowp setup` and `vowp secret`
> commands below are planned for v0.2 and are not available in a released binary yet.
> `vowp --version`, `vowp doctor`, `vowp init` and `vowp chat` are current. Nothing on this page
> asks for, prints or stores a secret value.

This page is written for the **setup agent**: the AI harness that the owner started with the
copyable prompt at the top of the root README. That prompt sends you here, at the exact release tag,
and tells you to follow this page exactly.

The owner is responsible for their VPS, its data and its bills. Your job is to run the installer
they approved, stop at the human hand-offs, and report what happened. You are not the VPS
administrator and you do not improvise around the plan.

## 1. What you are setting up

VOWP v0.2 has one **home** and, usually, one **control machine**:

| Term | Meaning in this contract |
|---|---|
| **home** | One Linux VPS that owns the global database, the project databases, the session logs and the supervisor. |
| **control machine** | The computer where the owner talks to the home: snapshot cache, SSH tunnels, the orchestrator surface. It holds no truth. |
| **project** | One git repository registered on the home. |
| **session** | One conversation with one id and an append-only log on the home. |
| **surface** | Where the owner is talking to a session now (desktop app, phone over SSH, terminal). |
| **orchestrator** | The long-lived session that reads the fleet snapshot and asks the owner before changing anything. |

The owner normally talks through the opencode desktop app or a Claude app. `vowp chat` is the
fallback surface. The setup agent does not replace either app; it installs the home, joins the
control machine, and proves both sides can reach each other.

This contract assumes you run on the computer that will be the control machine, with an SSH
connection to the home. If the owner is running you on the home itself, or on a machine that cannot
reach the home with an SSH key, stop and ask before continuing.

## 2. Ground rules

These rules apply before every command in this page.

1. **Ask once.** Ask all the questions in §3 in one message. Do not drip questions phase by phase.
   If an answer is incomplete, ask only for the missing part.
2. **One plan, one yes.** Show the whole plan from §4 once, then ask for one approval. Do not ask
   for a yes per phase. The only change before that approval is the local CLI bootstrap in §4.1,
   and you tell the owner about it in the same question message. If an answer changes, re-run the
   plan and ask again: that is a new plan.
3. **No secret values in chat, ever.** Values never appear in the conversation, in a command
   argument, in an environment variable you set, in a file you write, or in your report. Names are
   fine. The safe paths are in §7.
4. **Do not work around a hand-off.** If setup exits 5 and asks for a human, stop and wait. A
   hand-off is part of the contract, not a failure.
5. **Never guess.** If the plan, the owner's answer and this page disagree, stop and ask. Do not
   invent a flag, a path or a command.
6. **Nothing public or destructive.** Do not push, publish, deploy, change repository visibility,
   open a public port, change DNS or firewall rules, create cloud or provider accounts or SSH keys,
   or delete data. Setup does not need any of those.
7. **Do not touch what you were not asked to set up.** Keep the owner's existing projects, tmux
   sessions, harness logins and data. Do not edit a database or a session log by hand.
8. **Report honestly.** A warning stays a warning and a failed check stays a failure. Never turn a
   failure into a success in your summary.

## 3. Step 1 — Ask once

### 3.1 Check the basics without changing anything

Confirm these before you ask the owner:

- You can run commands on this computer.
- `ssh` is available.
- v0.2 setup documents Linux homes on `x86_64` or `arm64`; macOS is supported as a control machine.
  If the owner's target is something else, stop and ask.

Once the owner gives you the target, key-only login must work:

```sh
ssh -o BatchMode=yes -o ConnectTimeout=10 <ssh-target> true
```

If this prompts for a password or fails, stop before the plan and show the owner §11, "SSH key in
5 minutes". Do not ask for a password and do not run `ssh-copy-id` yourself; the owner must do that
step.

### 3.2 Ask the five questions

Send one message. Keep the five numbered questions, but write them in the owner's language and
style. Ask for **names**, not values.

```text
I can set up VOWP <version> for you. Before I show you the plan, I need five answers:

1. Home SSH target. Which `user@host` (or `~/.ssh/config` alias) should I use for the VPS?
   You need to log in with an SSH key, not a password; I will check that without changing anything.
2. Harnesses and providers. Which harness(es) should the workers, verifiers, planner and keeper
   use — `opencode`, `claude-code`, or both — and which provider name does each use? Names only;
   never send me a key.
3. Repositories. Which git repositories should I register on the home? Give a URL, or a path that
   already exists on the home. Say if a repository still needs `vowp init`.
4. GitHub remote. Should merged work be pushed to a GitHub remote so it is safe off the VPS?
   Recommended, never required.
5. Control machine. Should this computer be a control machine (snapshot cache, tunnels and the
   orchestrator surface)? Default yes.

If `vowp` is not already installed on this computer, I will install the pinned <version> binary
under `~/.local/bin` (no service, no cron) so I can show you the plan. That is the only change
before you approve the plan.
```

If the owner has no preference for question 2, the v0.1 default is `opencode` with DeepSeek; offer
that as the default and let the owner confirm it. Do not choose a provider for them.

If the owner has no repository yet, accept that answer. The projects phase can finish with no
projects; `vowp setup verify` uses its own fixture project and does not need the owner's repository.

### 3.3 What not to ask

- Do not ask for a password, a private key, an API key, a token, a `.env` file or a harness login.
  Those are hand-offs (§7), not questions.
- Do not ask for the VPS provider's console password or the root password.
- Do not ask a question that this page already answers.
- Do not ask the owner to choose a flag spelling or a path inside `~/.vowp`; the setup plan owns
  those details.

## 4. Step 2 — Bootstrap the CLI, then check without changing anything

### 4.1 Bootstrap the local CLI

The setup commands run on the control machine. If `vowp` is not already on `PATH`, install the
same pinned version from the release the prompt names. The owner was told about this line in §3.2.
It installs the per-user binary and its watchdog script under `~/.local/bin`, and enables no
service and no cron.

```sh
if ! command -v vowp >/dev/null 2>&1; then
  curl -fsSL "https://raw.githubusercontent.com/syver777/vowp/v<version>/scripts/install.sh" \
    | sh -s -- --download --version <version> --no-service --no-cron
fi
command -v vowp >/dev/null 2>&1 || export PATH="$HOME/.local/bin:$PATH"
vowp --version
```

The last command must print `vowp <version>` for the version in the prompt. If it prints a different
version, stop and report the mismatch; do not continue with a mixed install. If the archive for this
OS or architecture does not exist, stop and ask; do not build from an unrelated checkout.

Tell the owner again in the plan summary that this local line is part of the setup, even though the
remote plan prints only the home phases.

### 4.2 Run the plan

```sh
vowp setup plan --target <ssh-target>
vowp setup plan --target <ssh-target> --json
```

The first form is for the owner; the second is for you to parse. Both change nothing. If your
harness can capture only one, run the text form first and then the JSON form; the command is
read-only and safe to repeat.

What the plan contains (spec §11.2 and §11.3):

- the target and the VOWP version;
- every phase, every command (argv, no shell interpolation) and every file it will touch;
- the `requires` entries: `sudo`, `network`, `login:<harness>`, `secret:<NAME>` (name only) and
  `owner-answer:<question>`;
- whether each phase is idempotent;
- the hand-offs it expects.

The only network access in `plan` is the SSH version probe
`ssh -o BatchMode=yes <ssh-target> vowp --version`. A missing `vowp` on the home is the install
phase, not an error.

### 4.3 Check the plan against the owner's answers

Before you show the plan, compare it with §3:

- Do the target and version match the prompt?
- Does the projects phase name the repositories the owner gave, and no others?
- Does the harness/provider list match the owner's answer?
- Are the hand-offs the ones the owner should expect?
- Does the plan contain any secret value, token or private key? It must not; only names.
- Does any command touch something outside the home, the control home, the installed binary and
  service, the system packages the plan lists, and the project repositories the owner named? If so,
  stop and ask. Setup does not change firewalls, DNS, public ports or other repositories.

If the plan and the answers disagree, show the owner the difference and stop. Do not run a plan
that does something they did not ask for.

## 5. Step 3 — Show the plan once, get one yes

Show the full text plan, not a summary that hides a phase. Then add a short plain-language summary:

```text
This is the whole plan for <ssh-target>. It has <n> phases and will:
- install VOWP <version> on the home;
- create the non-root `vowp` user if sudo is available, or use no-sudo mode;
- create the home, the global database, the service and the vault (or basic secret mode);
- register <repositories>;
- set up this computer as a control machine.
It writes <n> files and runs <n> commands. It needs: <hand-offs>.
No secret values are in the plan; it names <secret names> only.

Run it? (yes/no)
```

Rules for the approval:

- One yes covers the whole plan. Do not ask "phase 1 ok? phase 2 ok?".
- If the owner says no, stop. Do not run a subset unless the plan itself has a documented way to
  do that; this contract does not.
- If the owner changes an answer, go back to §4, run the plan again, and ask for a new yes.
- If the owner asks a question the plan does not answer, stop and say the plan is incomplete. Do
  not guess and do not continue on a maybe.

## 6. Step 4 — Run the plan

### 6.1 Run the home phase

Run this from the control machine. Do not wrap it in `ssh`; the command opens the connection
itself. Do not prefix it with `sudo`; the setup creates the non-root user if it can.

```sh
vowp setup home --target <ssh-target> --apply --yes
```

Add `--json` if you need to parse the result instead of the text output. `--yes` is the single
approval from §5; never pass it before the owner has said yes.

`setup home` is idempotent. Re-running it after a hand-off continues from the phase that stopped
and does not redo finished work. Do not run `vowp up` or `vowp down` yourself; setup starts the
service.

The phases are:

| Phase | What it does | Changes |
|---|---|---|
| **preflight** | SSH key login, OS/arch, disk, `git`, `tmux`, `curl`. Never reads a secret. | Nothing. |
| **user** | Creates the non-root `vowp` user if sudo is available; otherwise runs as the current user and records no-sudo mode. | A Unix user, or a recorded mode. |
| **install** | Runs `scripts/install.sh --download` and verifies the installed version. | The `vowp` binary and its service files. |
| **home** | Creates `~/.vowp/`, the global database, the node registry, and runs `vowp doctor`. | The home directory and database. |
| **service** | Installs the user systemd unit `vowp.service`, enables linger, starts it. | A user service. |
| **vault** | Creates the vault user and helper when sudo allows; otherwise uses basic secret mode. Never stores a secret value. | A vault user/helper, or a recorded mode. |
| **projects** | Registers the repositories the owner named. Each must already have `vowp.toml`; `vowp init` is offered, not forced. | Project registry rows and project databases. |
| **tunnels** | None by default. The screenshot/browser case is a later owner request. | Nothing by default. |

The result is a `SetupOutcome` with `status`, `phase`, `changed`, `handoff` and `checks`. Treat any
`handoff` as a stop; go to §7.

### 6.2 Run the control phase

Run this only after the home phase exits 0 and only if the owner answered yes to question 5.

```sh
vowp setup control --apply
```

It writes `~/.vowp-control/` (the control config, the snapshot cache and the tunnel record) and
does not install a supervisor. If the shipped command asks for the home target, give it the same
`<ssh-target>`; this draft does not invent a flag (see §13). Never point it at a different machine.
If the owner declined a control machine, skip this step and say so in the report; the control-side
checks in §8 are then not verified.

### 6.3 Read the result

- Exit 0: the phase or the whole command succeeded. Continue.
- Exit 5 with `{"status":"handoff",...}`: stop and do §7.
- Exit 5 without a hand-off: a guard refused. Report the message and hint; do not work around it.
- Exit 7: SSH or remote failure. Check the target and the key, but do not retry blindly. Report it.
- Any other non-zero exit: stop. Show the error text or JSON (`error.message`, `error.hint`) and do
  not hand-fix the phase with your own commands.

Do not hide warnings. They belong in the report exactly as printed.

## 7. Hand-offs the human must do

The setup stops at a hand-off and exits 5 with a `handoff` object (`phase`, `kind`, `prompt`). Relay
the prompt to the owner in plain words, wait for an explicit confirmation, then re-run the **same**
command. Never start a new plan unless the hand-off changed one of the five answers.

| Hand-off | What the owner does | What you do | Never |
|---|---|---|---|
| **sudo password** | Runs the printed command in their own terminal on the home. You do not watch or capture the password. | Give the exact command; wait for "done"; re-run setup. | Ask for the password in chat; store it; echo it; put it in a file. |
| **harness login** | Opens the harness's device-code URL or signs in on the home with the harness's own flow. | Start the harness's own login on the home in a TTY if the plan asks you to; relay the URL or code; wait; re-run setup. | Ask for the account password or token; copy `~/.claude` or opencode credential files. |
| **secret value** | Enters it with `vowp secret set NAME` (hidden prompt) or `vowp secret request NAME` (one-time link). | Tell them the name and the safe method; wait for "saved"; check the name and last-4 only. | Ask them to paste it in chat; put it in argv, an environment variable or a file; read `~/.vowp/secrets`. |
| **owner answer** | Answers the `owner-answer:<question>` from the plan. | Use the answer; re-run the plan if it changes the plan. | Guess; pick a default the plan did not state. |

A sudo hand-off is not a request to become root. The owner runs the one printed command on the
home; you wait. A harness login is not a request for the owner's account password; the device-code
flow is enough. A secret is entered only through the hidden prompt or the one-time page; you see
`saved, ends in …last4`, never the value.

If the owner pastes a secret value into the chat anyway, do not repeat it, do not store it, and
tell them to rotate it and use the safe path.

## 8. Step 5 — Verify

Run this on the control machine after the home and control phases succeed:

```sh
vowp setup verify
```

`--json` is available if you need to parse each check. Verify prints each check as `ok`, `warn` or
`fail`, and exits 1 if any check fails. It runs a fixture project on the home; it does not touch
the owner's real projects.

The checks that matter:

- a fixture job is added, runs one turn, passes its gates and is merged by the merge queue;
- `snapshot.json` is readable from the control machine, with an `as of` time;
- an orchestrator session is created and answers through `vowp chat`;
- the gc dry run that verify prints first does not propose deleting production, truth, open work or
  secrets.

If a check fails, stop and report it. Do not edit the result, re-run until it passes without
understanding why, or call it a success because the warning is small. If the owner declined a
control machine, the snapshot check cannot be run from this computer; say that it was not verified
instead of claiming it passed.

## 9. Step 6 — Report

Give the owner a short report in the chat. If they ask for a file, write it outside any git
repository; never commit it.

```text
VOWP <version> setup — <home name or ssh target>

Home: <host> (<os>/<arch>), home at <path>, supervisor: <running/stopped>
Control: <this computer>, ~/.vowp-control, snapshot as of <time or "not verified">
Projects: <names and paths, or "none yet">
Harnesses/providers: <names only>
Secrets: <names and mode (vault user or basic), last-4 only if shown>
Verify: <n> ok, <n> warn, <n> fail; exit <code>
Running now: <supervisor, orchestrator session, tunnels>
Warnings: <exact text, or "none">
Next for you: <join the orchestrator from the desktop or phone; add a project; set a provider key;
             configure a GitHub remote; anything that needs the owner>
```

Rules for the report:

- Give versions of the local `vowp`, the home `vowp`, `git` and `tmux`, and the OS/arch of the home.
- Count the checks; do not paste a wall of JSON.
- Say what is running and how the owner reaches it.
- Put the owner's next actions last and make them concrete.
- Keep secret values, tokens, private keys, full `.env` contents and harness credentials out of the
  report. Names and last-4 only.
- If something was skipped, say it was skipped and why.
- If a warning or failure remains, say so in the first paragraph, not only in a table.

## 10. Stop conditions and refusals

Stop and ask the owner when:

- the SSH key login does not work in batch mode;
- the local CLI cannot be installed, or its version does not match the prompt;
- the target is not a Linux VPS, or its architecture is unsupported;
- `setup plan` exits 5 because it cannot proceed;
- the plan differs from the owner's answers, or contains something they did not ask for;
- a phase exits with an error other than a hand-off;
- `setup verify` has a failure;
- the owner asks for a command outside the plan, or the next step is ambiguous.

Refuse, and explain why, when asked to:

- put a secret, password, token or private key in the chat, an argument, an environment variable or
  a file in the repository;
- read `~/.vowp/secrets`, `.env`, SSH private keys or harness credential stores;
- edit a database, a session log, a capsule or a report by hand;
- run as root, change the firewall or DNS, open a public port, or change SSH server hardening;
- push, publish, deploy, change repository visibility, or create cloud or provider accounts or SSH
  keys;
- kill a tmux server, a supervisor or another owner's process;
- install software outside the plan (including extra harnesses or system packages).

A refusal is a stop, not a detour. Put the reason in the report and let the owner decide.

## 11. SSH key in 5 minutes

Give this section to the owner if the batch-mode check fails. The owner runs these commands; the
setup agent does not handle private keys.

```sh
# 1. Create a key if you do not already have one. Skip if you do.
ssh-keygen -t ed25519 -C "vowp-home"

# 2. Copy the public key to the home. This asks for the login password once.
ssh-copy-id <user>@<host>

# 3. Prove key-only login works. This must not ask for a password.
ssh -o BatchMode=yes -o ConnectTimeout=10 <user>@<host> true && echo "key login ok"
```

If `ssh-copy-id` is not installed, print the public key with `cat ~/.ssh/id_ed25519.pub` and append
it to `/home/<user>/.ssh/authorized_keys` on the home. On the home, the directory should be mode
0700 and the file mode 0600. On a cloud provider, the public key can be added in the provider's
console when the instance is created. If the instance already exists and password login is disabled,
use the provider's console to add the public key; do not re-enable password login just for setup.

An SSH config alias keeps the target short and makes the key explicit:

```sshconfig
Host vowp-home
  HostName <host>
  User <user>
  IdentityFile ~/.ssh/id_ed25519
  IdentitiesOnly yes
```

Then use `vowp-home` as `<ssh-target>`. Do not use `sshpass`, do not store a password in a script,
and do not send the private key anywhere.

## 12. The `vowp setup` commands (reference)

These are the commands this contract uses. The target is `--target`, not `--home`: `--home` is
already the global node-home flag, so the setup commands do not reuse it (spec §16, item 15).

```sh
vowp setup plan --target <ssh-target> [--json]
vowp setup home --target <ssh-target> --apply [--json] [--yes]
vowp setup control --apply [--json]
vowp setup verify [--json]
```

| Command | Changes anything? | Notes |
|---|---|---|
| `setup plan` | No. | Prints every phase, command and file; the only network is the SSH version probe. A missing remote `vowp` is the install phase, not an error. |
| `setup home --apply` | Yes. | Idempotent. Runs the eight phases in §6.1. A hand-off exits 5. |
| `setup control --apply` | Yes, on this computer. | Writes `~/.vowp-control/`; no supervisor. Run after the home succeeds. |
| `setup verify` | Runs a fixture on the home. | Prints `ok|warn|fail` per check; exits 1 on a fail, 0 otherwise. |

The JSON shapes are the spec's `SetupPlan` (`target`, `version`, `phases`, `handoffs`),
`SetupPhase` (`id`, `title`, `commands`, `files`, `requires`, `idempotent`), `SetupHandoff`
(`phase`, `kind`, `prompt`), `SetupOutcome` (`status`, `phase`, `changed`, `handoff`, `checks`) and
`SetupCheck` (`name`, `status`, `detail`). A plan never contains a secret value, a token or a
private key; `requires` names a secret by `<NAME>` only.

Relevant exit codes:

| Code | Meaning here |
|---|---|
| 0 | Success. |
| 1 | `setup verify` has a failed check. |
| 2 | Bad arguments or a missing `--target`; a plan cannot parse a config. |
| 5 | Hand-off needed, `--yes` missing, or the plan cannot proceed. Stop and follow the message. |
| 7 | SSH or remote failure on the control machine. |

The full CLI contract is in the [v0.2 specification](../design/v0.2/home-and-surfaces.md) §11 and
§12. This page is the user-facing contract; where the two differ, the specification wins and this
page is corrected.

## 13. Open details in this draft

These are the details the release implementation must settle; the setup agent must not improvise
them.

- **Local bootstrap.** The specification's §11.2 starts at `setup plan` and assumes `vowp` is
  already on the control machine. This draft adds the pinned release-installer line in §4.1. The
  implementation should make the plan cover the local bootstrap, or the published page should keep
  this line. A clean control machine has no `vowp`, so one of the two is required.
- **Answer plumbing.** The §11.2 synopsis has no flags for repositories, harnesses or providers.
  The implementation must decide whether `setup plan`/`home` prompt for them or take flags, and the
  plan must print the values it will use. This draft says "answer the command's prompts" and
  deliberately does not invent flag names.
- **`setup control` target.** The synopsis is `setup control --apply` with no target, while
  `control init` needs an SSH target. The implementation must make `setup control` reuse the target
  from the home run or `control.json`, or add a flag and amend the specification. The published
  page then documents the exact invocation.
- **`--yes`.** The exit-code table lists "missing `--yes`" as a refusal, so this draft treats
  `--yes` as the single approval flag. The implementation should confirm that reading and make the
  interactive form equally safe.
- **No control machine.** If the owner answers no to question 5, `setup verify` must define whether
  it runs on the home, skips the snapshot check with a warning, or refuses. This draft reports the
  check as not verified rather than claiming a pass.
- **Final version and README placement.** Before publication, replace `<version>` in both the
  prompt and the URL, put the prompt in the first README section, change this page's status from
  planned/draft to current, and remove the sentences that say the commands are not released. Do
  that after `vowp setup` is implemented and `setup verify` passes.
- **SSH key guide.** This draft keeps the five-minute guide inline (§11). If it moves to its own
  page, update every link so the documentation link check passes.
