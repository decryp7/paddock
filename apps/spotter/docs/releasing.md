# Releasing Spotter (and signed updates)

A `spotter-v*` tag on `main` runs `.gitea/workflows/spotter-release.yml` (repo root): Spotter's legs of `ci.yml` gate the tag
(Rust on Linux, Windows and macOS, the frontend with the Chromium and WebKit e2e), then the shared `app-release.yml` builds the
installers (Windows NSIS, the macOS `.dmg` sealed ad hoc and verified, Linux `.deb` and AppImage) and publishes a Gitea release
with the installers, release notes (`RELEASE_NOTES.md`, from git-cliff) and `latest.json`, the release information Spotter's
updater reads. It also puts `latest.json` on the moving `spotter-latest` release, which installed copies read. Installers are also built by the nightly run (`nightly.yml`) and by manual runs (`workflow_dispatch`, no release; `gates: true`
also runs Spotter's `ci.yml` legs first).

```sh
git tag spotter-v1.0.0 && git push origin spotter-v1.0.0
```

The version in the tag must match `app/src-tauri/tauri.conf.json` and the workspace `Cargo.toml`. Pre-releases are
not offered to users: updaters read `latest.json` of the *latest* release.

This is about **Spotter's own** updates, not the releases Spotter watches for.

## How Spotter's updates work

- **Opt-in.** Spotter never checks on its own unless the user turns on automatic update checks (off by default); a
  manual check always works. (The checking UI is not built yet; the updater plugin is wired and the app reports
  whether the build can update.)
- **One endpoint.** A check is one HTTPS GET of
  `https://dev.decryptology.net/decryp7/paddock/releases/download/spotter-latest/latest.json` (the moving `spotter-latest` release of the Paddock monorepo; copies installed before the move are reinstalled once by hand)
  (`plugins.updater.endpoints` in `tauri.conf.json`). It carries no identifier or usage data.
- **Signatures are mandatory.** Every update is verified with the minisign public key built into the app before it
  is installed, and `requireSignedVersion` binds each signature to its version (GHSA-j38x-g3m3-95fr). A build without
  the public key says "Updates are not available in this build" and never checks.
- `latest.json` is written by the shared `conformance/release/latest-json.mjs` from the signed installers (`*.sig` next to
  each); `node --test conformance/test/latest-json.test.mjs` (repo root) tests it.

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
`{"bundle":{"macOS":{"hardenedRuntime":true}}}` (notarization needs it). the shared `conformance/release/macos-verify-bundle.sh` (the `macos-verify` action) fails the
build when an ad-hoc Mach-O has the hardened runtime on or when bundled Mach-Os disagree on their TeamIdentifier, on the `.app`
and on the copy inside the DMG. Spotter bundles no sidecar today; one added to `bundle.externalBin` is started from the built app
by the same step (and `conformance/test/ci-actions.test.mjs` fails until `spotter-release.yml` lists it).

## Before tagging (rule 54)

- [ ] **`spotter-release.yml` dispatch with bundles green** (Actions > spotter-release > Run workflow, `gates` on): open the run and check that the three `bundle (...)` jobs ran and passed and that the verdict job says `installers: built`. A green run that says `installers: not built` proves nothing about the release path (rule 54).
- [ ] **`scripts/vm-smoke` passes on a fresh clone**: `source ~/.zshenv; scripts/vm-smoke/vm-smoke.sh --app spotter --run <the run id above>` (one VM at a time, local only). Attach its result file to the release notes or the tagging PR; a failed step blocks the tag.

See [scripts/vm-smoke/README.md](../../../scripts/vm-smoke/README.md) for what the check covers and what stays a manual row.

## First launch note

The apps are not signed or notarized: updates are protected by the family update key, and paid certificates would be needed to remove the OS warnings (decision 2026-11-05). The release workflow appends a "First launch" section to every release's notes (and so to `latest.json`) explaining **Open Anyway** (macOS) and **More info > Run anyway** (Windows SmartScreen).

<!-- paddock:block releasing rev=b41eea16 -->
## Releasing from a tag

Releases are made by CI from the tag `spotter-vX.Y.Z`, never from a workstation (rule 9). A local build for you to try is
fine; it is never published.

1. Make sure `main` is green in CI and the version in `tauri.conf.json`, `Cargo.toml` and `package.json` agree. The release
   notes are generated from the commits since the previous `spotter-v*` tag (git-cliff); there is no committed changelog
   file to update.
2. Before tagging (rule 25): `cargo deny check advisories`, one CI build with bundles (the workflow's manual run), the
   icon check, signing secrets stripped of whitespace, and the release notes' "First launch" section (until the apps are
   signed).
3. Request the tag, never push it: run the `tag` workflow (`.gitea/workflows/tag.yml`, `workflow_dispatch` with `tag` =
   `spotter-vX.Y.Z` and `sha` = the full commit id; API: `POST /api/v1/repos/decryp7/paddock/actions/workflows/tag.yml/dispatches`
   with `{"ref":"main","inputs":{"tag":"...","sha":"..."}}`). It checks that the commit is on origin, is not `[skip ci]`, that
   the tag is new, that the version matches and that the app's workflow is green on exactly that commit, then the release
   bot pushes an annotated tag. Tag protection refuses any other pusher. This is a guard rail against mistakes, not a security boundary: any repo writer can
   dispatch from their own ref and repo secrets reach non-fork runs (single-user setup, ER-37). Only a full CI run counts (a
   push to main, or a run whose plan job logged `plan mode: full=true`), and the commit must be on `main` or `release/*`. The tag workflow runs the gates
   again, builds the installers on all platforms, verifies the macOS bundle (`codesign --verify --deep --strict` on the .app
   and the DMG), publishes the release with its notes and `latest.json`.
4. After the release: install the published build (not the CI artifact) and run the app's installed-app verify script.
<!-- /paddock:block releasing -->

<!-- paddock:block first-launch rev=e18d3f38 -->
### First launch

Spotter is not code-signed yet, so your system may warn the first time you open it.

- **macOS:** first drag Spotter from the disk image to Applications and open it from there. The first time, macOS says "“Spotter” Not Opened": choose **Done** (not Move to Trash). Open System Settings > Privacy & Security, scroll down to "“Spotter” was blocked to protect your Mac." and choose **Open Anyway**. macOS asks once more, "Open “Spotter”?": choose **Open Anyway** again, then enter an administrator's name and password. You do this once; later starts open normally. If the button is missing, open Spotter again first.
- **Windows:** if SmartScreen says "Windows protected your PC", choose **More info > Run anyway**.
<!-- /paddock:block first-launch -->
