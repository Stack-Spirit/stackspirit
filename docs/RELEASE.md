# StackSpirit Desktop installers — release workflow

This is the runbook for the native StackSpirit Desktop installers built by
[`.github/workflows/desktop-release.yml`](../.github/workflows/desktop-release.yml).
The runtime CLI/daemon artifacts remain owned by
[`.github/workflows/release.yml`](../.github/workflows/release.yml); this workflow only adds the
desktop installers for the same `v<version>` tag.

## 1. What is built

| Runner | Rust target | Installer |
|---|---|---|
| `macos-14` | `aarch64-apple-darwin` | `StackSpirit-<version>-aarch64.dmg` |
| `macos-15-intel` | `x86_64-apple-darwin` | `StackSpirit-<version>-x86_64.dmg` |
| `ubuntu-22.04` | `x86_64-unknown-linux-gnu` | `StackSpirit-<version>-x86_64.AppImage` and `StackSpirit-<version>-x86_64.deb` |

The matrix is native, not cross-compiled. Every leg fetches the pinned StackSpirit Engine
sidecar for its own target with `node scripts/fetch-engine.mjs --target <triple>`, then runs the
Tauri build from `apps/spirit-desktop` with the bundles shown above. The Linux leg installs the
Tauri system dependencies (WebKitGTK 4.1, GTK 3, appindicator, librsvg, `patchelf`, FUSE) before
building.

The workflow stages those files under `dist/desktop/`, then a single `checksums` job collects
them, writes `SHA256SUMS-desktop`, verifies the sums, and uploads the complete set as the
`desktop-installers` workflow artifact. A failed matrix leg stops the job, so a partial installer
set is never checksummed or published.

## 2. Version and names

The installer version comes from the release tag, without the leading `v`:

| Run | Version |
|---|---|
| `push` of `v0.6.1` | `0.6.1` |
| `release: published` for `v0.6.1` | `0.6.1` |
| `workflow_dispatch` with `tag: v0.6.1` | `0.6.1` |
| `workflow_dispatch` with an empty `tag` | the checked-in `apps/spirit-desktop/src-tauri/tauri.conf.json` version |

The build writes a one-line `tauri.release.conf.json` override before `tauri build`, so the
DMG, plist, AppImage and `.deb` metadata all report the resolved version. The checked-in
`tauri.conf.json` version is the development default, not a second release version. Tags must
produce a valid semver (`X.Y.Z`, optionally with a pre-release/build suffix); otherwise the
`prepare` job fails before any expensive build starts.

## 3. Triggers and release attachment

| Trigger | What happens |
|---|---|
| Push of a `v*` tag | Builds and checksums all installers. Tries to attach them to an existing release for that tag; if no release exists yet, the run still succeeds and keeps the `desktop-installers` artifact. |
| A release is published | Builds again and attaches the installer set to that published release, replacing same-named assets (`gh release upload --clobber`). This is the reliable attach path. |
| `workflow_dispatch`, empty `tag` | Dry build only. No release is read or written. Download the `desktop-installers` artifact from the run. |
| `workflow_dispatch` with `tag` | Checks out that tag, builds it, and attaches only when a GitHub Release for the tag exists. A missing release is a notice, not a failure. |

Every build checks out the resolved tag (or the selected ref for a dry dispatch), so the
installers match the tagged source rather than whatever is currently on the default branch.

`release: created` is deliberately not used: the runtime Release workflow creates its draft with
the repository `GITHUB_TOKEN`, and GitHub does not start new workflow runs from events caused by
that token. Publishing the draft is the owner action that reliably starts this workflow.

This workflow never creates a release, never publishes one, and never edits release notes. It
only uploads desktop assets to a release that already exists. Upload errors are real failures;
"no release yet" is not.

## 4. Running a build

### Dry build from GitHub Actions

1. Open **Actions → Desktop Release → Run workflow**.
2. Leave `tag` empty.
3. When the run finishes, download the **desktop-installers** artifact and verify it locally.

The artifact contains the four installers (two DMGs, one `.AppImage` and one `.deb`) and
`SHA256SUMS-desktop`.

### Rebuilding and attaching an existing release

1. Open **Actions → Desktop Release → Run workflow**.
2. Enter the existing tag, for example `v0.6.1`. The tag must already exist; the checkout
   fails cleanly otherwise.
3. The run checks out that tag, rebuilds the matrix, and attaches the assets if the release
   exists. Re-running replaces the desktop assets from the earlier attempt; it does not touch
   the CLI, npm, PyPI or wheel assets.

A dry build without a tag is the safe way to test the workflow, because it cannot write to a
release even if a tag with the same version exists.

### Normal release order

1. Land the desktop change on `main` and push the `v<version>` tag.
2. `.github/workflows/release.yml` builds the runtime artifacts and creates a **draft** release.
3. `.github/workflows/desktop-release.yml` builds the desktop installers. If the draft already
   exists it attaches them; if not, it leaves them in the workflow artifact and succeeds.
4. The owner reviews the draft and publishes it.
5. Publishing starts the desktop workflow again; this run attaches the installer set to the
   published release. Confirm `StackSpirit-<version>-*.dmg`, `.AppImage`, `.deb` and
   `SHA256SUMS-desktop` are present before announcing the release.

## 5. Verifying downloads

Verify the checksums from the directory that contains the downloaded files:

```sh
# macOS
shasum -a 256 -c SHA256SUMS-desktop

# Linux
sha256sum --check --strict SHA256SUMS-desktop
```

GitHub release assets do not preserve the Unix executable bit. On Linux, make the AppImage
executable after downloading it:

```sh
chmod +x StackSpirit-<version>-x86_64.AppImage
./StackSpirit-<version>-x86_64.AppImage
```

The Debian package declares its WebKitGTK/GTK runtime dependencies; install it with `apt` so they
are resolved:

```sh
sudo apt install ./StackSpirit-<version>-x86_64.deb
```

## 6. macOS is unsigned — the Gatekeeper notice is real

There is **no Apple Developer ID certificate, no notarization and no stapling** in this workflow.
The macOS DMGs are ad-hoc/unsigned builds. On first launch macOS will show a Gatekeeper warning
such as “Apple could not verify ‘StackSpirit’ is free of malware”. This is expected, not a
corrupt download.

To open the app without disabling Gatekeeper globally:

1. Drag **StackSpirit** from the DMG to `/Applications`.
2. Try to open it once, then go to **System Settings → Privacy & Security** and choose
   **Open Anyway** for StackSpirit; or Control-click the app in Finder and choose **Open**.
3. Confirm the checksum above before opening an installer from a release you did not build.

Do not tell users to run `spctl --master-disable` or to turn Gatekeeper off. The proper fix is an
Apple Developer ID certificate with `codesign --options runtime` and `notarytool` submission,
which is a separate owner account and release-hardening task, not a workflow default.

## 7. Limits

- **Unsigned everywhere.** macOS is unsigned and not notarized. Windows is not built at all.
  Linux packages are unsigned as well.
- **No auto-update.** Tauri updater signing and update endpoints are not part of this workflow.
- **No Linux arm64.** This workflow covers `x86_64` Linux only.
- **No owner live smoke.** The workflow proves packaging and checksums; it does not prove a real
  SSH host, OS-keychain round trip or live daemon/Web smoke. The owner checklist remains in
  [`OWNER-VERIFICATION.md`](OWNER-VERIFICATION.md).
- **Artifacts expire.** Workflow artifacts follow the repository retention setting; attached
  release assets persist with the release.
- **Desktop version versus runtime version.** The runtime release workflow checks
  `runtime/Cargo.toml`; this workflow stamps the tag version into the desktop bundle. The two
  versions are intentionally taken from the same tag so users see one StackSpirit release
  version, but a tag is not proof that the desktop app passed the runtime gates.

For a local build, see [`DEV-SETUP.md`](DEV-SETUP.md). The local path produces an unsigned
bundle with the checked-in version; it is not a substitute for the three-runner release matrix.
