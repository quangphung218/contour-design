# Releasing

How a version of Contour goes from `main` to a published GitHub Release.
CI does the building; a person decides when and checks the result.

## Versioning

- Semantic Versioning. Before 1.0, a minor bump (`0.2.0`) may break things;
  a patch bump (`0.1.1`) only fixes them.
- **One source:** `[workspace.package] version` in the root `Cargo.toml`.
  Every crate inherits it, the daemon reports it (`od --version`,
  `GET /api/health`), and the desktop bundle reads it because
  `apps/desktop/tauri.conf.json` has no `version` of its own.
- Tags are `v<version>`, e.g. `v0.2.0`. The release workflow refuses a tag
  that does not match `Cargo.toml`.

## Steps

1. **Start from a green `main`.** CI on `main` runs on Linux only, so pull
   and run the full local check on macOS:

   ```bash
   git switch main && git pull
   make ci
   ```

2. **Review the changelog.** Everything user-visible since the last release
   should be under `## [Unreleased]` in [CHANGELOG.md](../CHANGELOG.md),
   grouped as Added, Changed, Fixed, Removed, Security.

3. **Bump the version** on a release branch:

   ```bash
   git switch -c release/v0.2.0
   make version V=0.2.0
   ```

   This rewrites the workspace version, refreshes `Cargo.lock`, turns
   `## [Unreleased]` into `## [0.2.0] - <today>` with a fresh empty
   Unreleased above it, and updates the compare links at the bottom. Check
   the notes it prints:

   ```bash
   scripts/release-notes.sh 0.2.0
   git diff
   ```

4. **Build locally once** to catch bundling problems before CI does:

   ```bash
   make app-release      # Contour.app, about 130 MB (UI WASM about 12 MB)
   make dmg              # Contour_0.2.0_aarch64.dmg (no Finder window layout locally)
   ```

5. **Commit and merge** through a pull request:

   ```bash
   git commit -am "build(release): v0.2.0"
   ```

6. **Tag the merge commit** on `main` and push the tag:

   ```bash
   git switch main && git pull
   git tag -a v0.2.0 -m "Contour v0.2.0"
   git push origin v0.2.0
   ```

7. **Watch the Release workflow** (`.github/workflows/release.yml`):

   | Job | Output |
   |---|---|
   | check | fails fast if the tag, `Cargo.toml` and `CHANGELOG.md` disagree |
   | macOS tests | `ci.yml` (`only: macos`): `make test` on macOS; bundles wait for it, so a failure stops the release |
   | Windows build (does not block) | `ci.yml` (`only: windows`): the Windows compile check; nothing waits for it, a failure shows on the run but the release goes on (Windows is best effort) |
   | bundle (macos-universal) | `Contour_<v>_universal.dmg` (Apple silicon and Intel) |
   | bundle (linux-x64) | `Contour_<v>_amd64.AppImage`, `Contour_<v>_amd64.deb` (built on Ubuntu 22.04) |
   | bundle (windows-x64) | `Contour_<v>_x64_en-US.msi`, best effort: a failure here does not block the release |
   | release | `SHA256SUMS` and a **draft** GitHub Release titled `Contour v<v>` with the CHANGELOG section as notes |

8. **Check the draft.** Download each file, verify it against `SHA256SUMS`,
   open the app, create a project and run one turn. Then press **Publish**
   on the draft.

To rebuild a release (for example after a flaky runner), run the workflow by
hand from the Actions tab with the existing tag. Files are replaced only
while the release is still a draft; a published release is never rewritten
(the job fails instead), so cut a new patch version.

## Signing and notarisation (macOS)

Optional. Without these secrets the workflow builds an **unsigned** bundle,
prints a warning and carries on. Users then allow it once: System Settings ▸
Privacy & Security ▸ Open Anyway on macOS 15 and later, Control-click ▸ Open
on macOS 14 and earlier, or `xattr -dr com.apple.quarantine
/Applications/Contour.app` (see getting-started.md).

Add them under Settings ▸ Secrets and variables ▸ Actions:

| Secret | Value |
|---|---|
| `APPLE_CERTIFICATE` | the Developer ID Application certificate exported as `.p12`, base64-encoded (`base64 -i cert.p12 \| pbcopy`) |
| `APPLE_CERTIFICATE_PASSWORD` | the `.p12` password |
| `APPLE_SIGNING_IDENTITY` | e.g. `Developer ID Application: Your Name (TEAMID)` |
| `APPLE_ID` | Apple ID used for notarisation |
| `APPLE_PASSWORD` | an app-specific password for that Apple ID |
| `APPLE_TEAM_ID` | the 10-character team id |

With the first three the bundle is signed; with all six it is also
notarised and stapled by the Tauri CLI. The values reach only the
environment of the "Tauri bundle" step (never `GITHUB_ENV` or other steps)
and are never printed.

To sign a local build, export the same variables before `make dmg`.

## Hotfixes

Branch from the release tag, fix, bump the patch version and tag:

```bash
git switch -c hotfix/v0.2.1 v0.2.0
# fix, commit (fix(<scope>): …), add a CHANGELOG entry under Unreleased
make version V=0.2.1
git commit -am "build(release): v0.2.1"
git tag -a v0.2.1 -m "Contour v0.2.1" && git push origin hotfix/v0.2.1 v0.2.1
```

Then merge the hotfix branch back into `main`.

## Installing a build locally

```bash
make app-release
make install          # replaces /Applications/Contour.app only if it is Contour
```

`make install` checks the bundle identifier (`dev.contour.desktop`) of an
existing `/Applications/Contour.app` and refuses to replace anything else.
Other apps in `/Applications` are never touched. `INSTALL_DIR=~/Applications
make install` installs somewhere else.
