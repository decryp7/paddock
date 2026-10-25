# Releasing Spotter (and signed updates)

A `v*` tag on `main` runs `.gitea/workflows/build.yml`: the `bundle` jobs build the installers (Windows NSIS, the macOS `.dmg` on the `macos-latest` host runner, and Linux `.deb` and AppImage), and the `release` job publishes a Gitea release with the
installers, release notes (`RELEASE_NOTES.md`, from git-cliff) and `latest.json`, the release information Spotter's
updater reads. Installers are also built for the nightly run of `main` and manual runs (`workflow_dispatch`).

```sh
git tag v1.0.0 && git push origin v0.2.0
```

The version in the tag must match `app/src-tauri/tauri.conf.json` and the workspace `Cargo.toml`. Pre-releases are
not offered to users: updaters read `latest.json` of the *latest* release.

This is about **Spotter's own** updates, not the releases Spotter watches for.

## How Spotter's updates work

- **Opt-in.** Spotter never checks on its own unless the user turns on automatic update checks (off by default); a
  manual check always works. (The checking UI is not built yet; the updater plugin is wired and the app reports
  whether the build can update.)
- **One endpoint.** A check is one HTTPS GET of
  `https://dev.decryptology.net/decryp7/spotter/releases/download/latest/latest.json`
  (`plugins.updater.endpoints` in `tauri.conf.json`). It carries no identifier or usage data.
- **Signatures are mandatory.** Every update is verified with the minisign public key built into the app before it
  is installed, and `requireSignedVersion` binds each signature to its version (GHSA-j38x-g3m3-95fr). A build without
  the public key says "Updates are not available in this build" and never checks.
- `latest.json` is written by `scripts/release/latest-json.mjs` from the signed installers (`*.sig` next to each);
  `node --test scripts/release/latest-json.test.mjs` tests it.

## One-time set-up: Spotter's own signing key

Spotter needs its **own** key pair — never reuse another product's updater key.

1. Generate a key pair on a trusted machine, with a strong password:

   ```sh
   cd app
   npx tauri signer generate -w ~/.tauri/spotter-updater.key
   ```

   This writes the private key `spotter-updater.key` (encrypted with the password) and the public key
   `spotter-updater.key.pub`.

2. Set three **repository secrets** in Gitea (Settings ▸ Actions ▸ Secrets) of `decryp7/spotter`:

   | Secret | Value |
   | --- | --- |
   | `TAURI_SIGNING_PRIVATE_KEY` | the contents of `spotter-updater.key` |
   | `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | its password |
   | `TAURI_UPDATER_PUBKEY` | the contents of `spotter-updater.key.pub` |

   Also `RELEASE_TOKEN`: a Gitea access token with `write:repository` scope, used to publish the release.

   The bundle jobs then build with `createUpdaterArtifacts` and the public key (passed as `tauri build --config`,
   and as `TAURI_UPDATER_PUBKEY` for `option_env!` in `app/src-tauri/src/updater.rs`). Without the secrets (forks,
   pull requests) the installers are built without update support and the release job publishes no `latest.json`.

   The committed `tauri.conf.json` keeps `pubkey` empty and does not set `createUpdaterArtifacts`, so local
   `npx tauri build` works without the private key.

3. **Back up the private key and its password** (password manager plus an offline copy). Never commit it, never put
   it anywhere the UI is built from.

**If the private key is lost**, no further update can be signed for the key built into installed copies: users
install the next version by hand. **If it leaks**, generate a new pair, publish a release signed with the new key,
tell users to reinstall from the releases page, and revoke the old secrets.

## The macOS bundle

The `.app` is sealed ad hoc (`"signingIdentity": "-"` in `app/src-tauri/tauri.macos.conf.json`): a bundle that is only
linker-signed has no sealed resources and a quarantined download is reported as damaged. While the build is ad hoc the
**hardened runtime is off** (`"hardenedRuntime": false`, Paddock rule 23): ad-hoc code under the hardened runtime
enforces library validation and cannot load a library signed by its vendor ("different Team IDs"). When the
`APPLE_SIGNING_IDENTITY` secret names a real identity, the CI bundle step passes
`{"bundle":{"macOS":{"hardenedRuntime":true}}}` (notarization needs it). `scripts/ci/macos-verify-bundle.sh` fails the
build when an ad-hoc Mach-O has the hardened runtime on, when bundled Mach-Os disagree on their TeamIdentifier, or
when a sidecar does not start from the built app (`--version`), on the `.app` and on the copy inside the DMG.

## First launch note

Apple notarization and Windows code signing are deferred until after the releases settle (decision 2026-10-23). Until then the release workflow appends a "First launch" section to every release's notes (and so to `latest.json`) explaining **Open Anyway** (macOS) and **More info > Run anyway** (Windows SmartScreen).
