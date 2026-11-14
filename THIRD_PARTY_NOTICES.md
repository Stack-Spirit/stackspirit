# Third-Party Notices

StackSpirit bundles one third-party application: `opencode`, presented to users as the
**StackSpirit Engine**. The binary is fetched at build time from the pinned release in
`engine/manifest.json`; it is not committed to this repository.

## opencode

- Project: opencode
- Pinned version: 1.19.5
- Source: https://github.com/anomalyco/opencode
- Release: https://github.com/anomalyco/opencode/releases/tag/v1.19.5
- License: MIT

The bundled sidecar is used under the MIT license below. StackSpirit's own code is also
MIT-licensed; see `LICENSE`.

```text
MIT License

Copyright (c) 2025 opencode

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

The About screen must keep the "StackSpirit Engine runs on opencode (MIT)" credit and a
link to this file.

## Rust and JavaScript dependencies

Rust crates and npm packages retain their own licenses. Their license texts are
available from the crate/package sources and from `Cargo.lock` / `package-lock.json`.
StackSpirit does not relicense them.
