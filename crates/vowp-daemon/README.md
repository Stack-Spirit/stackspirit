# vowp-daemon

The read-only loopback HTTP API for one VOWP project.

`vowp-daemon [--home <dir>] [--project <name>] [--port <n>]` opens an existing VOWP home
read-only, serves `GET /api/v1/health`, `/api/v1/board`, `/api/v1/jobs`, `/api/v1/sessions`
and `/api/v1/sessions/{id}/events`, and binds `127.0.0.1` only. The default port is `0`, so
the operating system picks an ephemeral port and the binary prints the bound address.

This is the first v0.3 slice: there is no token, no write endpoint and no SSE. The listener
is loopback-only and nothing writes, so there is no privileged action to authenticate. A
token must be added before a write endpoint or a non-loopback listener.

The API serves the same `vowp-protocol` objects the CLI prints. Spec:
[`docs/design/v0.3/http-api.md`](../../docs/design/v0.3/http-api.md).
