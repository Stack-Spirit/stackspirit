# Documentation tests

Two offline checks guard the user documentation. They are the acceptance tests for the documentation
job and can run in CI without network access.

| Script | What it proves |
|---|---|
| [`check-links.sh`](check-links.sh) | Every relative Markdown link resolves to a file, and heading anchors in the pages this job owns exist. External URLs are not fetched. |
| [`check-commands.sh`](check-commands.sh) | Every `vowp` command shown in `README.md`, `docs/concepts/**`, `docs/guides/**`, `docs/security.md` or `docs/troubleshooting.md` exists in [`docs/design/cli.md`](../design/cli.md). The command surface is parsed from the design file; a required-set assertion catches parser drift. |
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

It does **not** validate flags, arguments or whether a command is implemented yet. Commands that are
not implemented are marked *(planned)* in the text; the job map removes those marks when the CLI lands.

## Status declaration

Every page that shows a `vowp` command must declare its status: either it contains the literal marker
`*(planned)*`, or it declares `Status: current` once the command is implemented. The checker fails a
page with commands but no declaration. This is the mechanism behind the job-map rule that commands stay
marked *(planned)* until the CLI merges and the marks are removed afterwards.

## Adding a command

1. Add or update the command in [`docs/design/cli.md`](../design/cli.md) first. That file is the
   contract; the documentation may not invent a command.
2. Use the command in a page under the owned paths.
3. Run `bash docs/tests/run.sh`.

If a command is genuinely new, the design change belongs in a `docs/design/**` job. Do not add it to the
checker's allow-list: there is no allow-list.
