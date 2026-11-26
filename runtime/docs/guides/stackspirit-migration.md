# Migrating a live home to StackSpirit

> **Status: current.** Every command below exists in the `stackspirit` binary. `vowp` is the
> compatibility alias with the same command tree, and it keeps working throughout v6.

This guide moves a running VOWP home, its agent nodes and the desktop/control machines to the
StackSpirit names **without stopping active work**. There is no database schema change and no wire
change, so the whole migration is a binary swap plus a link: nothing is moved, nothing is
re-enrolled and no session log is rewritten. The design and its reasoning are in the
[rebrand plan](../design/stackspirit-engine-rebrand.md); the support window for the old names is in
the [deprecation policy](../deprecation-policy.md).

**VOWP compatibility.** `vowp`, `vowp-daemon`, `vowp.toml`, `~/.vowp`, `~/.vowp-agent`,
`~/.vowp-control`, `VOWP_HOME` and the `vowp` MCP key all keep working. A fresh host uses the new
names; an existing host keeps its data and units in place. This private tree does not change the
public VOWP repository, its archive names or its tags.

## What changes and what does not

| Changes (primary name) | Stays (supported alias or frozen name) |
|---|---|
| `stackspirit` CLI, `stackspirit-daemon` | `vowp`, `vowp-daemon` installed beside them |
| `stackspirit.toml` for new private repositories | `vowp.toml` is read when there is no `stackspirit.toml` |
| `~/.stackspirit`, `~/.stackspirit-agent`, `~/.stackspirit-control` for new hosts | Existing `~/.vowp*` directories are used in place |
| `STACKSPIRIT_HOME` is exported with `VOWP_HOME` | `VOWP_HOME` is still read and exported |
| `stackspirit` MCP key for **new** orchestrators | Existing orchestrators keep the `vowp` key |
| — | `.vowp/`, `vowp.db`, session logs, HTTP routes, MCP tool names, JSON tags |

## 1. Back up and record

On the home, before anything else:

```sh
vowp backup
cp -a ~/.vowp ~/vowp-pre-stackspirit-backup
vowp --version
vowp doctor
```

Keep the filesystem copy until you are satisfied. Rollback never edits a database; the copy is
for the optional directory move.

## 2. Install the bridge binaries beside the old ones

Save the current binaries first; they are the rollback copies. The `vowp` and `vowp-daemon` links
are not optional: the old systemd unit, agent transport, `--node` commands and stored
orchestrator configs invoke them.

```sh
cp -a ~/.local/bin/vowp ~/.local/bin/vowp.pre-stackspirit
cp -a ~/.local/bin/vowp-daemon ~/.local/bin/vowp-daemon.pre-stackspirit
install -m 0755 stackspirit ~/.local/bin/stackspirit.new
mv ~/.local/bin/stackspirit.new ~/.local/bin/stackspirit
ln -sfn stackspirit ~/.local/bin/vowp
install -m 0755 stackspirit-daemon ~/.local/bin/stackspirit-daemon.new
mv ~/.local/bin/stackspirit-daemon.new ~/.local/bin/stackspirit-daemon
ln -sfn stackspirit-daemon ~/.local/bin/vowp-daemon
stackspirit --version
vowp --version
stackspirit --home ~/.vowp doctor
```

If `~/.local/bin/vowp` was a symlink, copy the real binary explicitly before replacing the link.
Do not continue until both `--version` lines answer and `doctor` passes against the old home.

## 3. Link bridge

The link bridge makes the new names resolve without moving data:

```sh
stackspirit home migrate --link --dry-run
stackspirit home migrate --link --yes
```

`--dry-run` prints the symlinks it would create and writes nothing. If `~/.stackspirit` already
exists as a **real** directory next to a real `~/.vowp`, the resolver refuses to pick one: stop and
resolve it by hand. Both names pointing at the same directory is fine.

## 4. Drain and replace the supervisor

`--replace` drains first, so no run, gate task or merge is killed, and admits no new work until the
new supervisor owns the lock. Do not use `systemctl restart` while work is active.

```sh
stackspirit up --replace
stackspirit status --check
vowp status --check
```

The home path, socket, session logs and `vowp.db` are unchanged, so tmux views stay valid. The
daemon child restarts and writes a fresh `web-window` address; web clients reconnect with their
existing backoff. Then verify:

```sh
stackspirit doctor
stackspirit fleet status
stackspirit orchestrator ls
```

`doctor` reports the legacy home or config as a warning, not an error, and `stackspirit` and `vowp`
status must agree.

## 5. Agent nodes

Do this on each Mac or Linux agent node. First stop or move any local orchestrator so a service
restart cannot drop a live app window; the home log is authoritative and the session survives a
move.

```sh
vowp agent status
vowp orchestrator move <name> --home
```

Then install both binaries as in section 2 and refresh the generated unit **in place**:

```sh
test -e ~/.stackspirit-agent || ln -s .vowp-agent ~/.stackspirit-agent
stackspirit agent service install --now
stackspirit agent status
stackspirit agent doctor
```

The installer updates the existing unit (`vowp-agent.service` on Linux, `dev.vowp.agent` on macOS) and exports both
`STACKSPIRIT_HOME` and `VOWP_HOME`. It does not create a second unit beside an enabled legacy one, and
no re-enrollment or surface re-pairing is needed. Renaming the unit to the StackSpirit label
(`stackspirit-agent.service` / `dev.stackspirit.agent`) is a separate, explicit step:

```sh
stackspirit agent service install --rename-unit --now
```

If both a legacy and a StackSpirit unit already exist, the command refuses; remove one by hand.

## 6. Desktop and control machines

Update StackSpirit Desktop and the control CLI to the new release. Desktop resolves `stackspirit`
first and `vowp` second, keeps the same tunnel and `/api/v1` endpoint, and never parses a home path
out of the snapshot. The control CLI updates in place; `control.json` keeps the legacy
`remote_home`, so these still work afterwards:

```sh
stackspirit control status
stackspirit fleet status
```

New orchestrators use the `stackspirit` MCP key; existing ones keep `vowp`. Verify one new
orchestrator's `tools/list` and one existing orchestrator's tool calls.

## 7. Fresh hosts and the installer

Run the installer in StackSpirit product mode only for fresh hosts, or when you explicitly want the
new names. On a host with legacy units it updates them in place: it never adds a second service, a
second cron line or a second watchdog. The public VOWP installer and archive names are unchanged.

## 8. Rollback

Rollback is a binary rollback because nothing else changed.

1. Move `vowp.pre-stackspirit` and `vowp-daemon.pre-stackspirit` back over the aliases, then
   restart with `vowp up --replace`. Old names and old configs are still present.
2. If an agent unit was renamed, disable the StackSpirit unit and reinstall the legacy one with
   the pre-StackSpirit binary (`vowp.pre-stackspirit agent service install --now`); the legacy unit
   is generated by the same installer.
3. If a new orchestrator used the `stackspirit` MCP key, the old engine still runs `vowp mcp`; the
   app may show the new key, so re-approve its tools or recreate the key. Session data is unchanged.
4. Never roll back by editing a database or deleting a session log.

Old binaries can **not** be promised to read a project that has only a `stackspirit.toml` and no
`vowp.toml`. Keep `vowp.toml` in any repository that an old binary must still open.

## 9. Optional: move the directory

Only after the bridge has run everywhere and you have a quiet window. Stop the supervisor, take a
backup, then:

```sh
stackspirit home migrate --move --to ~/.stackspirit --dry-run
stackspirit home migrate --move --to ~/.stackspirit --yes
```

The move copies, verifies and swaps, then leaves a legacy symlink at the old path and prints the
socket-preservation line. It is a maintenance operation, not part of the rebrand: the link bridge
already gives the new name. After a move, rollback means stopping the new supervisor, restoring the
pre-move copy (or pointing the new path back at the backup) and starting the old binary.
