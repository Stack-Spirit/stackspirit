# stackspirit (npm)

StackSpirit is a local-first supervisor for AI coding agents. This package is a thin launcher: it
contains no binary of its own and runs no install script. npm installs exactly one prebuilt
platform package (`@stackspirit/<os>-<cpu>`) next to it, and `lib/launch.js` runs the binary from
there.

## Use

```sh
npx -y stackspirit --version
npx -y stackspirit mcp
npm install -g stackspirit
```

`stackspirit mcp` with no flags is a **read-only** stdio MCP server against your local StackSpirit
home. It lists five read tools (`fleet_status`, `fleet_queue`, `inbox_list`, `capacity_report`,
`session_tail`) and answers any write tool with JSON-RPC error `-32601`. The full server, which can
write, needs `--session`, `--surface` and `--token-file` together.

Requires Node.js 18 or newer. The package works with `npm ci --ignore-scripts`.

## Commands

The package installs four commands: `stackspirit`, `stackspirit-daemon`, and the legacy names
`vowp` and `vowp-daemon`. The binary chooses its product identity from the name it is started as.

## Supported platforms

| Platform | Package |
|---|---|
| Linux x64 (glibc) | `@stackspirit/linux-x64` |
| Linux arm64 (glibc) | `@stackspirit/linux-arm64` |
| macOS x64 | `@stackspirit/darwin-x64` |
| macOS arm64 | `@stackspirit/darwin-arm64` |

Linux with musl libc (for example Alpine) and Windows are not supported by this package. On
Windows use WSL 2; on Alpine build from source. The launcher prints that message and exits 1.

If you installed with `--omit=optional`, the launcher reports the missing platform package; run
`npm install` again without that flag.

## Migrating from the `vowp` npm package

The old `vowp` package is frozen. Replace it:

```sh
npm uninstall -g vowp
npm install -g stackspirit
```

`vowp` and `vowp-daemon` keep working: this package provides them. Your `~/.vowp` data is not
touched.

## License

MIT
