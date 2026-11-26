# stackspirit

StackSpirit: a local-first supervisor for AI coding agents. This distribution is a set of
self-contained platform wheels; each carries the `stackspirit`, `vowp`, `stackspirit-daemon` and
`vowp-daemon` binaries and four console scripts that exec them. There is no source distribution.

```sh
uvx stackspirit --version
pip install stackspirit
```

## Supported platforms

| Wheel tag | Minimum |
|---|---|
| `manylinux_2_35_x86_64` | glibc 2.35 (Ubuntu 22.04) |
| `manylinux_2_35_aarch64` | glibc 2.35 |
| `macosx_10_12_x86_64` | macOS 10.12 |
| `macosx_11_0_arm64` | macOS 11 |

musl, Windows and 32-bit systems have no wheel; build from source with a Rust toolchain instead.

## Building the wheels (maintainers)

```sh
bash scripts/release/build-pypi-wheels.sh --version 0.6.0 --target-dir target --out dist-pypi
```

Publishing is an owner action and is not done by this repository's scripts.
