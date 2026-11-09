# Documentation tests

Two offline checks guard the user documentation. They are the acceptance tests for the documentation
job and can run in CI without network access.

| Script | What it proves |
|---|---|
| [`check-links.sh`](check-links.sh) | Every relative Markdown link resolves to a file, and heading anchors in the pages this job owns exist. External URLs are not fetched. |
| [`check-commands.sh`](check-commands.sh) | Every `vowp` command shown in `README.md`, `docs/concepts/**`, `docs/guides/**`, `docs/security.md` or `docs/troubleshooting.md` exists in the real CLI, and every command that is not implemented yet is marked *(planned)*. The command surface comes from [`docs/design/cli.md`](../design/cli.md) and `V0_1_COMMANDS`; implementation status comes from the CLI dispatch table. A required-set assertion catches parser drift. |
| [`run.sh`](run.sh) | Runs both checks and prints one pass line. |

Run them from the repository root:

```sh
bash docs/tests/run.sh
```

Or individually:

```sh
bash docs/tests/check-links.sh
bash docs/tests/check-commands.sh
```

## What the command check understands

- Backticked invocations in prose and tables: `` `vowp status --check` ``.
- Fenced shell blocks, with or without a leading `$ `.
- The workspace form `cargo run -p vowp-cli --bin vowp -- <command>`.
- Global flags before the command, including the value-taking `--node`, `--project` and `--home`.
- Family subcommands (`job`, `session`, `inbox`, `node`, `schedule`), which are checked as two words.
- Trailing shell comments. A `#` without preceding whitespace is not a comment, so executor variants
  such as `opencode:deepseek/deepseek-flash#max` survive intact.

It does **not** validate flags or arguments. It does distinguish a command that only appears in
`--help` from one whose handler is still the explicit `NotImplemented` stub: a documented command whose
top-level handler is a stub must be marked *(planned)* in the page or in the section before it. The
checker reads `crates/vowp-cli/src/cli.rs` and `crates/vowp-cli/src/cmd/` to make that distinction.

## Status declaration

Every page that shows a `vowp` command must declare `Status: current`, `Status: planned` or
`Status: mixed`. A planned page covers all its commands; a mixed page must mark each not-yet-implemented
command or section with the literal marker `*(planned)*`. The checker fails a page with commands but no
declaration, and fails an unmarked command whose handler is still a stub. When the CLI lands, remove the
mark and move the page to `current` or `mixed`.

## Adding a command

1. Add or update the command in [`docs/design/cli.md`](../design/cli.md) first. That file is the
   contract; the documentation may not invent a command.
2. Use the command in a page under the owned paths.
3. Run `bash docs/tests/run.sh`.

If a command is genuinely new, the design change belongs in a `docs/design/**` job. Do not add it to the
checker's allow-list: there is no allow-list.
