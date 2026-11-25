# Legacy name deprecation policy

> **Status: current.** This is a policy, not a command reference.

StackSpirit is the primary product name. The original VOWP names are **supported for all of v6**.

| Legacy name | v6 behaviour |
|---|---|
| `vowp` binary | Installed beside `stackspirit`; identical command tree, flags, exit codes and JSON. |
| `vowp-daemon` | Installed beside `stackspirit-daemon`; both are discovered by sibling name. |
| `vowp.toml` | Read when there is no `stackspirit.toml`; a conflict is reported, never silently resolved. |
| `~/.vowp`, `~/.vowp-agent`, `~/.vowp-control` | Used in place when no StackSpirit directory exists; data is never moved automatically. |
| `VOWP_HOME` and related variables | Still read and still exported alongside the `STACKSPIRIT_*` names. |
| Legacy systemd/launchd units and watchdog | Updated in place by the installer; never duplicated. |
| The `vowp` MCP key | Kept by existing orchestrators; only new orchestrators use `stackspirit`. |
| `.vowp/`, `vowp.db`, wire formats, routes, MCP tool names | Frozen. They are data and protocol, not branding. |

Rules:

- No v6 release prints a "removed" error for a legacy name.
- Removing any legacy name is a **v7 decision**. It requires that stored orchestrator configs and
  agent transports have been migrated, and it needs its own migration plan.
- The public VOWP repository, its archive names and its tags are not changed by this policy.

See the [migration guide](guides/stackspirit-migration.md) for the upgrade sequence and the
[rebrand plan](design/stackspirit-engine-rebrand.md) for the reasoning.
