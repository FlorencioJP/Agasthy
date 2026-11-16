# Releasing

Releases are built by `.github/workflows/release.yml` when a tag `vX.Y.Z` is pushed. The tag must match `version` in `apps/desktop/src-tauri/tauri.conf.json`. The workflow runs the tests, builds the app and DMG with `pnpm release:mac`, writes the updater manifest with `scripts/updater-manifest.mjs`, and creates a **draft** GitHub release. Publishing the draft makes it visible to the in-app updater.

## One-time setup

1. **Updater signing key.** Installed copies only accept updates signed with the private key whose public half is in `tauri.conf.json` (`plugins.updater.pubkey`). The key was generated with `pnpm tauri signer generate` and lives at `~/.tauri/agasthy-updater.key` on the maintainer's machine. **Never commit it.** Add its contents as the repository secret `TAURI_SIGNING_PRIVATE_KEY`. If the key is lost, existing installs can no longer be updated automatically; keep a backup in a password manager.
2. **Apple signing (optional).** Without it, users right-click → Open the first time. With an Apple Developer account, add these secrets and the workflow signs and notarizes automatically: `APPLE_CERTIFICATE` (base64 of a Developer ID Application `.p12`), `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`, `APPLE_ID`, `APPLE_PASSWORD` (an app-specific password) and `APPLE_TEAM_ID`.

## Cutting a release

```sh
# 1. bump "version" in apps/desktop/src-tauri/tauri.conf.json (and the CHANGELOG)
# 2. commit, then tag and push
git tag v0.2.0 && git push origin main v0.2.0
# 3. review the draft release on GitHub, then publish it
```

## Building locally

```sh
pnpm release:mac                                   # unsigned DMG in dist/
TAURI_SIGNING_PRIVATE_KEY="$(cat ~/.tauri/agasthy-updater.key)" pnpm release:mac
node scripts/updater-manifest.mjs --repo HMAKT99/Agasthy   # dist/latest.json + update archive
```
