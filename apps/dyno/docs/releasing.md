# Releasing Dyno (and signed updates)

A `dyno-v*` tag on `main` runs `.gitea/workflows/dyno-release.yml` (repo root): first the gates (Dyno's legs of `ci.yml`, WebKit e2e included), then the shared `app-release.yml`, whose `bundle` jobs build the installers (Windows NSIS on the
`windows-latest` host runner, the macOS `.dmg` on the `macos-latest` host runner, and a `.deb` plus AppImage on a Linux
container), and the `release` job publishes a Gitea release with the installers, release notes (`RELEASE_NOTES.md`, from
git-cliff) and `latest.json`, the release information Dyno's updater reads. Installers are also built for the nightly run
of `main` and manual runs (`workflow_dispatch`), and are uploaded as workflow artifacts (the v3 artifact actions, which
Gitea Actions supports).

```sh
git tag dyno-v0.2.0 && git push origin dyno-v0.2.0
```

The version in the tag must match `app/src-tauri/tauri.conf.json` and the workspace `Cargo.toml`. Pre-releases are not
offered to users: updaters read `latest.json` of the *latest* release. The `release` job needs every other job to pass
(lint, tests on Linux, Windows and macOS, licences, frontend, browser flows, all three bundles).

## What a build contains

- **Windows:** `Dyno_<version>_x64-setup.exe` (NSIS, per-user).
- **macOS:** `Dyno_<version>_aarch64.dmg`, and with signing secrets also the `.app.tar.gz` the updater installs. The bundle is
  **ad-hoc signed** (`signingIdentity "-"` in `tauri.macos.conf.json`): `SMAppService` and launch agents refuse a
  linker-signed bundle. It is not notarised: users open it once with right-click > Open. Developer ID signing and
  notarisation are future work.
  While the build is ad hoc the **hardened runtime is off** (`"hardenedRuntime": false`, Paddock rule 23): ad-hoc code
  under the hardened runtime enforces library validation and cannot load a library signed by its vendor ("different
  Team IDs"). When the `APPLE_SIGNING_IDENTITY` secret names a real identity, the CI bundle step passes
  `{"bundle":{"macOS":{"hardenedRuntime":true}}}` (notarization needs it). the shared `macos-verify` action (`conformance/release/macos-verify-bundle.sh`) fails
  the build when an ad-hoc Mach-O has the hardened runtime on, when bundled Mach-Os disagree on their TeamIdentifier,
  or when a sidecar does not start from the built app (`--version`), on the `.app` and on the copy inside the DMG.
- **Linux:** `Dyno_<version>_amd64.deb` and an AppImage.
- Every bundle carries the `dyno-agent` and `dyno` (command line, docs/cli.md) sidecars (`externalBin`, built by `app/src-tauri/scripts/sidecar.mjs` with the `dist`
  profile). On macOS the bundle also carries `Contents/Library/LaunchAgents/net.decryptology.dyno.agent.plist`.

`scripts/check-bundle.sh` runs after the Linux and macOS bundles (`bundle-check` in `dyno-release.yml`): every executable's run paths must be relative or system
folders (never a build directory), the macOS seal must verify, and `dyno-agent` must be present. Dyno 1.0 ships without its privileged helper: the `dyno-helper` crate stays in the workspace for 1.1 but is not built into or bundled with any installer, and no installer is expected to contain it.

## How Dyno's updates work

- **Opt-in.** Dyno checks for updates only when the user turns the check on; a manual check always works.
- **One endpoint.** A check is one HTTPS GET of
  `https://dev.decryptology.net/decryp7/paddock/releases/download/dyno-latest/latest.json` (the moving `dyno-latest` release of the Paddock monorepo; copies installed before the move are reinstalled once by hand) (`plugins.updater.endpoints` in
  `tauri.conf.json`). It carries no identifier or usage data.
- **Signatures are mandatory.** Every update is verified with the minisign public key built into the app before it is
  installed. A build without the public key says "Updates are not available in this build" and never checks.
- `latest.json` is written by the shared `publish-release` action (`conformance/release/`) from the signed installers (`*.sig` next to each); its tests are in `conformance/`.

## One-time set-up: Dyno's own signing key

Dyno needs its **own** key pair; never reuse another product's updater key.

1. Generate it on a trusted machine, with a strong password:

   ```sh
   cd app
   npx tauri signer generate -w ~/.tauri/dyno-updater.key
   ```

2. Set four **repository secrets** in Gitea (Settings > Actions > Secrets) of `decryp7/dyno`:

   | Secret | Value |
   | --- | --- |
   | `TAURI_SIGNING_PRIVATE_KEY` | the contents of `dyno-updater.key` |
   | `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | its password |
   | `TAURI_UPDATER_PUBKEY` | the contents of `dyno-updater.key.pub` |
   | `RELEASE_TOKEN` | a Gitea access token with `write:repository` scope, used to publish the release |

   With the first three set, the bundle jobs build with `createUpdaterArtifacts` and the public key (passed as
   `tauri build --config`). **Without them (forks, pull requests, a fresh repository) every build still works**: installers are
   produced unsigned and without update support, and the release job publishes no `latest.json`. The committed
   `tauri.conf.json` keeps `pubkey` empty, so local `npx tauri build` works without the private key.

3. **Back up the private key and its password** (password manager plus an offline copy). Never commit it.

**If the private key is lost**, no further update can be signed for the key built into installed copies: users install the
next version by hand. **If it leaks**, generate a new pair, publish a release signed with the new key, tell users to
reinstall from the releases page, and revoke the old secrets.

## Before tagging

- `main` is green (all jobs of `build.yml`).
- `CHANGELOG`/release notes read well: `git cliff --latest` (Conventional Commits; no attribution trailers).
- The overhead numbers in `docs/architecture.md` were re-measured on a quiet machine for this release (roadmap G13).
- `docs/screenshots` match the built screens (`npm run screenshots` in `app/`).

## First launch note

Apple notarization and Windows code signing are deferred until after the releases settle (decision 2026-10-23). Until then the release workflow appends a "First launch" section to every release's notes (and so to `latest.json`) explaining **Open Anyway** (macOS) and **More info > Run anyway** (Windows SmartScreen).
