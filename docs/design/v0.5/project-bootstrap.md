# v0.5 — one-step project bootstrap

Status: **implemented** for v0.5 Slice 4. `vowp project add` can clone a repository,
initialise a checkout that has no `vowp.toml`, register the project and create its
orchestrator, then print one summary. The design scope is in
[`scope.md`](scope.md) §4; the command contract is in
[`../cli.md`](../cli.md) §6.1.

## Command

```text
vowp project add [<path>] [--git <url>] [--init] [--orchestrator <app>] [--name <name>] [--weight <1-100>]
```

At least one of `<path>` or `--git <url>` is required; clap enforces it. The legacy
`vowp project add <path> [--name <name>] [--weight <1-100>]` form keeps its human output
and its `ProjectEntry` JSON exactly. A run is in **bootstrap mode** when `--git`, `--init`
or `--orchestrator` is present.

- `<path>` is the checkout to register. With `--git` it is the explicit clone target
  instead.
- `--init` initialises the checkout only when `vowp.toml` is absent. An existing file is
  never read as input to a rewrite and is left byte-identical; the summary reports
  `initialized: false`.
- `--orchestrator <app>` creates the project orchestrator for `opencode`,
  `claude-code`/`claude_code`/`claude`, or `chat`. `chat` has no host and no keeper.
- `--name` and `--weight` keep their existing meaning.

## Clone target rules

With `--git`, the project name is `--name` or the URL basename with a trailing `.git`
removed. The name is parsed with `ProjectName::parse`; `_node` is refused.

The target is the explicit positional path when given, else `<home-parent>/<name>`,
where `<home-parent>` is `NodePaths::root().parent()` (for `~/.vowp`, the user's home).
Before any clone, the command refuses:

- a missing home parent;
- an existing target, including a broken symlink;
- a target containing a `..` component;
- a target whose canonical parent is outside the canonical home parent;
- a URL with userinfo in its `scheme://` authority, such as
  `https://user:password@host/repo.git` or `https://user@host/repo.git`.

SCP-style `git@host:path` is allowed: `git` is the SSH user, not a URL credential, and
that form carries no password. A URL with `@` after the authority, such as
`https://host/team@repo`, is not treated as credentialed.

## Clone execution

The clone is argv-only: `git clone -- <url> <target>` through
`std::process::Command`, never a shell and never a password in argv. The child gets:

| Variable | Value |
|---|---|
| `GIT_TERMINAL_PROMPT` | `0` |
| `GIT_CONFIG_GLOBAL` | `/dev/null` |
| `GIT_CONFIG_NOSYSTEM` | `1` |
| `GIT_SSH_COMMAND` | `ssh -oBatchMode=yes` |

On failure the command removes only the target it created, then returns a nonzero error.
The URL is replaced with `<url>` in git's stderr and any remaining `scheme://userinfo@`
span is redacted, so neither the raw URL nor a credential is echoed. Credential helpers,
SSH key management and interactive login are out of scope.

## Init and register order

The order is fixed:

1. validate the credential rule and target;
2. clone when `--git` is present;
3. when `--init` is present and `vowp.toml` is absent, run the same initializer as
   `vowp init` (the extracted `init::run_at`) at the git checkout root;
4. register through `register_checkout`, the same function `vowp up` and `vowp init`
   use;
5. when `--orchestrator` is present, create or reuse the project orchestrator.

`init` and `register_checkout` each apply the registry name/path conflict rules before
writing. Registration happens before the orchestrator step, so an orchestrator failure
never removes a registered project.

## Orchestrator step

The orchestrator is named after the project (the existing default) and is bound to that
project. There is no node target in this command; `vowp orchestrator move` owns placement.

- An existing orchestrator with the same name, project and app is reused, including its
  session id. The command starts the detached keeper only when the app needs a home host
  (`opencode`/`claude-code`); `chat` never starts one.
- Any other existing orchestrator with that name is a conflict: a different app or a
  different project is never retargeted silently.
- If the step fails after registration, the project stays registered, the command exits
  nonzero, and a newly created orchestrator directory is stopped and removed. The error
  names the exact retry:

  ```text
  vowp orchestrator new --name <project> --app <app> --project <project>
  ```

## Summary

In bootstrap mode `--json` prints exactly one object:

```json
{"project":"vowp","path":"/home/user/vowp","initialized":true,"orchestrator":null}
```

`orchestrator` is the full `OrchestratorCard` when one was created or reused, else
`null`. `initialized` is true only when this command ran the initializer. Human output is
one line naming the project, path, `initialized yes|no` and the orchestrator name/state or
`none`. The legacy non-bootstrap output is unchanged.

## Tests

Integration tests (existing CLI test targets; no new `[[test]]` entry):

- `project_add_without_new_flags_keeps_the_legacy_json` (`tests/up.rs`) — the legacy
  `ProjectEntry` key set and values are unchanged.
- `project_add_git_clones_initializes_registers_and_returns_one_summary`
  (`tests/init.rs`) — a local bare repository is cloned to `<home-parent>/<name>`,
  `vowp.toml` is written, the registry row exists, the summary has exactly
  `project`, `path`, `initialized` and `orchestrator`, and the chat card names the same
  project and session.
- `project_add_git_refuses_an_existing_target_and_a_url_with_credentials`
  (`tests/init.rs`) — no clone, no registry row, and no credential or raw URL in
  stdout/stderr.
- `project_add_init_is_skipped_when_vowp_toml_exists` (`tests/init.rs`) — the file stays
  byte-identical and `initialized` is false.
- `project_add_git_failure_leaves_no_partial_checkout_or_registry_row`
  (`tests/init.rs`) — a missing local source leaves no target and no project row.
- `project_add_orchestrator_reuses_a_matching_existing_card_and_refuses_a_conflict`
  (`tests/init.rs`) — the second chat run reuses the same card and session, the chat app
  starts no keeper, and a different app conflicts.
- `project_add_orchestrator_failure_keeps_the_project_and_names_the_retry`
  (`tests/init.rs`) — a conflicting orchestrator name leaves the project registered and
  the error names the retry command.
- `project_add_bootstrap_summary_never_contains_the_url_or_a_secret` (`tests/init.rs`) —
  a marker in the source URL path is absent from the summary and stderr.

Unit tests in `crates/vowp-cli/src/cmd/project.rs`:

- `clone_argv_is_argv_only` — the argv is exactly `clone -- <url> <target>`.
- `url_credentials_are_refused_in_scheme_authorities_only` — `scheme://user@` and
  `scheme://user:password@` are refused, SCP-style and path `@` are not, and redaction
  removes the userinfo.
- `url_basename_strips_scp_syntax_and_a_git_suffix` — URL and SCP basenames become a
  project name.

## Out of scope

Credential helpers, SSH key management and interactive login; a node target for the
orchestrator step; multi-project batch add; project templates; a second registry format.
