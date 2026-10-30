# Releasing Pitcrew (and signed updates)

A `pitcrew-v*` tag on `main` runs `.gitea/workflows/pitcrew-release.yml` (repo root): Pitcrew's legs of `ci.yml` gate the tag
(Rust on Linux, Windows and macOS, the frontend), then the shared `app-release.yml` builds the installers on three platforms
(Windows NSIS, the macOS `.dmg` sealed ad hoc and verified, Linux `.deb` and AppImage) and publishes a Gitea release with the
installers, release notes (`RELEASE_NOTES.md`, from git-cliff) and `latest.json`, the release information Pitcrew's
updater reads. It also puts `latest.json` on the moving `pitcrew-latest` release, which installed copies read. Installers
are also built by the nightly run (`nightly.yml`) and by manual runs (`workflow_dispatch`, no release).

```sh
git tag pitcrew-v0.2.0 && git push origin pitcrew-v0.2.0
```

The version in the tag must match `app/src-tauri/tauri.conf.json` and the workspace `Cargo.toml`. Pre-releases are
not offered to users: updaters read `latest.json` of the *latest* release.

This is about **Pitcrew's own** updates. The runners Pitcrew manages are updated by the service from their
projects' releases (ADR 0002), independently of this.

## How Pitcrew's updates work

- **Opt-in.** Pitcrew never checks on its own unless the user turns on automatic update checks (off by default); a
  manual check always works. (The checking UI is not built yet; the updater plugin is wired and the app reports
  whether the build can update.)
- **One endpoint.** A check is one HTTPS GET of
  `https://dev.decryptology.net/decryp7/paddock/releases/download/pitcrew-latest/latest.json` (the moving `pitcrew-latest` release of the Paddock monorepo; copies installed before the move are reinstalled once by hand)
  (`plugins.updater.endpoints` in `tauri.conf.json`). It carries no identifier or usage data.
- **Signatures are mandatory.** Every update is verified with the minisign public key built into the app before it
  is installed, and `requireSignedVersion` binds each signature to its version (GHSA-j38x-g3m3-95fr). A build without
  the public key says "Updates are not available in this build" and never checks.
- `latest.json` is written by the shared `conformance/release/latest-json.mjs` from the signed installers (`*.sig` next to
  each); `node --test conformance/test/latest-json.test.mjs` (repo root) tests it.

## One-time set-up: Pitcrew's own signing key

Pitcrew needs its **own** key pair — never reuse another product's updater key.

1. Generate a key pair on a trusted machine, with a strong password:

   ```sh
   cd app
   npx tauri signer generate -w ~/.tauri/pitcrew-updater.key
   ```

   This writes the private key `pitcrew-updater.key` (encrypted with the password) and the public key
   `pitcrew-updater.key.pub`.

2. Set three **repository secrets** in Gitea (Settings ▸ Actions ▸ Secrets) of `decryp7/Pitcrew`:

   | Secret | Value |
   | --- | --- |
   | `TAURI_SIGNING_PRIVATE_KEY` | the contents of `pitcrew-updater.key` |
   | `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | its password |
   | `TAURI_UPDATER_PUBKEY` | the contents of `pitcrew-updater.key.pub` |

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

## Before tagging (rule 54)

- [ ] **`pitcrew-release.yml` dispatch with bundles green** (Actions > pitcrew-release > Run workflow, `gates` on): open the run and check that the three `bundle (...)` jobs ran and passed and that the verdict job says `installers: built`. A green run that says `installers: not built` proves nothing about the release path (rule 54).
- [ ] **`scripts/vm-smoke` passes on a fresh clone**: `source ~/.zshenv; scripts/vm-smoke/vm-smoke.sh --app pitcrew --run <the run id above>` (one VM at a time, local only). Attach its result file to the release notes or the tagging PR; a failed step blocks the tag.

See [scripts/vm-smoke/README.md](../../../scripts/vm-smoke/README.md) for what the check covers and what stays a manual row.

## First launch note

Apple notarization and Windows code signing are deferred until after the releases settle (decision 2026-10-23). Until then the release workflow appends a "First launch" section to every release's notes (and so to `latest.json`) explaining **Open Anyway** (macOS) and **More info > Run anyway** (Windows SmartScreen).
