# stackspirit (npm, private variant)

The private npm package for **StackSpirit**, the local-first supervisor for AI
coding agents (formerly VOWP). It installs the prebuilt binaries for your
platform: `stackspirit` and `stackspirit-daemon`, plus the legacy `vowp` and
`vowp-daemon` commands, which behave the same way under the old names.

The package name is `stackspirit`. It is marked `"private": true`, so
`npm publish` refuses to run: **nothing here is published**. Publishing is an
owner action outside the repository's jobs. The public `vowp` package in
`../npm` is separate and unchanged; do not install both globally, because both
provide a `vowp` command.

## How it installs

The postinstall step downloads two files for package version `X.Y.Z`:

```text
$STACKSPIRIT_RELEASE_BASE_URL/vX.Y.Z/stackspirit-X.Y.Z-<target>.tar.gz
$STACKSPIRIT_RELEASE_BASE_URL/vX.Y.Z/stackspirit-X.Y.Z-<target>.tar.gz.sha256
```

There is no default URL: the archive is private, so
`STACKSPIRIT_RELEASE_BASE_URL` must be set (HTTPS, or plain HTTP only for
loopback). The step verifies the SHA-256, then extracts exactly
`stackspirit`, `vowp`, `stackspirit-daemon` and `vowp-daemon` into `vendor/`.
An archive that lacks any of the four is rejected and nothing is installed.

`STACKSPIRIT_SKIP_DOWNLOAD=1` skips the download.

## Tests

```sh
node --test packaging/npm-stackspirit/test/*.test.js
```

The tests are offline: a local loopback server serves a fake release.
