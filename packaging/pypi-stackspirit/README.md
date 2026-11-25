# stackspirit (PyPI, private variant)

A launcher package for **StackSpirit** (formerly VOWP). It ships no Rust code:
a platform wheel carries the four prebuilt binaries from the private archive
`stackspirit-<version>-<target>.tar.gz` under `src/stackspirit/bin/`, and the
console scripts `stackspirit`, `vowp`, `stackspirit-daemon` and `vowp-daemon`
run them.

The distribution name is `stackspirit`. The `Private :: Do Not Upload`
classifier makes PyPI reject an upload; nothing is published from this
repository.

`STACKSPIRIT_BIN_DIR` overrides the directory the launcher looks in, which is
how the offline tests run it.

```sh
python3 -I -m unittest discover -s packaging/pypi-stackspirit/tests
```
