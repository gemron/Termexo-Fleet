# Release & Publishing Guide (GitHub Actions + npm)

Termexo Fleet ships cross-platform desktop installers and an npm CLI launcher (`termexo-fleet`) automatically through GitHub Actions.

## Workflows

| Workflow | File | Triggers | What It Does |
| --- | --- | --- | --- |
| **CI** | `.github/workflows/ci.yml` | Push / PR to `main` | Builds Angular UI, runs Vitest + npm launcher tests, and runs `cargo test --workspace` on Ubuntu 22.04, macOS 14, and Windows |
| **Release Desktop & npm** | `.github/workflows/release.yml` | Git tag `v*` or manual `workflow_dispatch` | Syncs versions, builds 4 desktop targets in parallel, generates `SHA256SUMS.txt` + `release-manifest.json`, publishes GitHub Release, and publishes `termexo-fleet` to npm |

## Release Matrix

| Target | Runner | Installers | Portable Archive (used by npm) |
| --- | --- | --- | --- |
| **Linux x64** | `ubuntu-22.04` (glibc 2.35 compatible) | `.deb`, `.rpm`, `.AppImage` | `termexo-fleet-linux-x64.tar.gz` |
| **macOS Apple Silicon** | `macos-14` (`aarch64-apple-darwin`) | `Termexo-Fleet_<ver>_macos-arm64.dmg` | `termexo-fleet-macos-arm64.tar.gz` |
| **macOS Intel** | `macos-14` (`x86_64-apple-darwin`) | `Termexo-Fleet_<ver>_macos-x64.dmg` | `termexo-fleet-macos-x64.tar.gz` |
| **Windows x64** | `windows-latest` (`x86_64-pc-windows-msvc`) | `*-setup.exe` (NSIS), `*.msi` | `termexo-fleet-windows-x64.zip` |

## How to Cut a Release

### 1. Stable Release (`latest` on npm)
```bash
node scripts/release-sync-version.mjs v0.2.0
git add -A
git commit -m "chore(release): v0.2.0"
git tag v0.2.0
git push origin main --follow-tags
```

### 2. Preview / Prerelease (`next` on npm)
Tags containing a hyphen (e.g. `v0.2.0-preview.1`) are automatically marked as GitHub **Pre-release** and published to npm under the `next` dist-tag:
```bash
node scripts/release-sync-version.mjs v0.2.0-preview.1
git add -A
git commit -m "chore(release): v0.2.0-preview.1"
git tag v0.2.0-preview.1
git push origin main --follow-tags
```

### 3. Dry-Run Verification (No Publish)
In GitHub → **Actions → Release Desktop & npm → Run workflow**, check **`dry_run = true`** to compile all 4 platforms and validate checksums + `npm pack` without publishing a GitHub Release or npm package.

## Configuring npm Publishing

The workflow supports **both** npm authentication methods automatically:

1. **OIDC Trusted Publishing (Recommended, Zero Long-Lived Secrets)**:
   - On [npmjs.com](https://www.npmjs.com/), open package **`termexo-fleet` → Settings → Trusted Publisher**.
   - Add GitHub Actions publisher:
     - **Owner**: `gemron`
     - **Repository**: `Termexo-Fleet`
     - **Workflow filename**: `release.yml`
   - Leave `NPM_TOKEN` unset in GitHub Secrets — the workflow publishes using `id-token: write` with `--provenance`.
2. **Classic Automation Token Fallback**:
   - If you prefer token auth (or before configuring Trusted Publishing on the very first publish), create an npm Granular Access Token with publish permission and save it as repository secret **`NPM_TOKEN`**.

## How `npx termexo-fleet` Works

`packages/npm/` publishes a lightweight Node.js (`>= 18.18`) launcher with zero third-party dependencies:
- Resolves the current OS/CPU (`windows-x64`, `macos-arm64`, `macos-x64`, `linux-x64`).
- Downloads the matching portable archive from GitHub Releases on first run.
- Strictly verifies its SHA-256 digest against the `release-manifest.json` baked into the npm package.
- Caches the extracted app in the OS user cache directory (`~/.cache/termexo-fleet`, `~/Library/Caches/termexo-fleet`, or `%LOCALAPPDATA%\termexo-fleet\Cache`) and launches the native Tauri desktop binary.
