# Security policy

## Supported versions

VOWP is pre-release software. Security fixes land on `main` and in the latest `0.1.x` pre-release; older
pre-release builds are not supported.

## Report a vulnerability privately

Please use GitHub's private vulnerability reporting for this repository:

<https://github.com/syver777/vowp/security/advisories/new>

Do not open a public issue, discussion or pull request for a vulnerability. In the report, include the
affected version or commit, the operating system, reproduction steps, and the impact you expect. Do not
include credentials or other secret values; refer to secrets by name only.

## What VOWP stores

VOWP stores **no credentials**. Harness logins stay in each harness (for example `~/.claude` or
opencode's own auth and environment). Provider keys are environment variables the user already has.
Logs and reports refer to secret names, never values. The supervisor and any local API bind to
`127.0.0.1`; remote access is expected to go through SSH or a tunnel.

## Scope

Reports about the VOWP crates, workflows and bundled framework files are in scope. Vulnerabilities in
third-party harnesses or model providers should be reported to those projects directly.
