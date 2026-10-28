# Releasing Dyno (and signed updates)

A `dyno-v*` tag on `main` runs `.gitea/workflows/dyno-release.yml` (repo root): first the gates (Dyno's legs of `ci.yml`, WebKit e2e included), then the shared `app-release.yml`, whose `bundle` jobs build the installers (Windows NSIS on the
`windows-latest` host runner, the macOS `.dmg` on the `macos-latest` host runner, and a `.deb` plus AppImage on a Linux
container), and the `release` job publishes a Gitea release with the installers, release notes (`RELEASE_NOTES.md`, from
git-cliff) and `latest.json`, the release information Dyno's updater reads. Installers are also built for the nightly run
of `main` and manual runs (`workflow_dispatch`), and are uploaded as workflow artifacts (the v3 artifact actions, which
Gitea Actions supports).

```sh
git tag dyno-vX.Y.Z && git push origin dyno-vX.Y.Z   # X.Y.Z = the version in tauri.conf.json
```

The version in the tag must match `app/src-tauri/tauri.conf.json` and the workspace `Cargo.toml`. Pre-releases are not
offered to users: updaters read `latest.json` of the *latest* release. The `release` job needs `plan`, `gates` (lint, tests on Linux, Windows and macOS, frontend, browser flows) and all three `bundle` jobs to pass; the licence and notices checks run in `plan`.

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
- Every bundle carries the `dyno-agent`, `dyno-helper` and `dyno` (command line, docs/cli.md) sidecars (`externalBin`, built by `app/src-tauri/scripts/sidecar.mjs` with the `dist`
  profile). On macOS the bundle also carries `Contents/Library/LaunchAgents/net.decryptology.dyno.agent.plist`.

`scripts/check-bundle.sh` runs after the Linux and macOS bundles (`bundle-check` in `dyno-release.yml`): every executable's run paths must be relative or system
folders (never a build directory), the macOS seal must verify, and `dyno-agent` and `dyno-helper` must be present. The helper is installed only on a user's click from the copy next to the agent
(ADR 0016); an update brings a new copy, and Settings > Helper then offers to update the installed one (one OS prompt).

## How Dyno's updates work

- **Opt-in.** Dyno checks for updates only when the user turns the check on; a manual check always works.
- **One endpoint.** A check is one HTTPS GET of
  `https://dev.decryptology.net/decryp7/paddock/releases/download/dyno-latest/latest.json` (the moving `dyno-latest` release of the Paddock monorepo; copies installed before the move are reinstalled once by hand) (`plugins.updater.endpoints` in
  `tauri.conf.json`). It carries no identifier or usage data.
- **Signatures are mandatory.** Every update is verified with the minisign public key built into the app before it is
  installed. A build without the public key says "Updates are not available in this build" and never checks.
- `latest.json` is written by the shared `publish-release` action (`conformance/release/`) from the signed installers (`*.sig` next to each); its tests are in `conformance/`.

## Signing key and secrets

Dyno uses the **family's shared updater key** (one minisign key pair for every Paddock app), kept as secrets of the
`decryp7/paddock` repository (Settings > Actions > Secrets), not as per-app secrets and not in `decryp7/dyno`:

| Secret | Value |
| --- | --- |
| `PADDOCK_TAURI_SIGNING_PRIVATE_KEY` | the contents of the private key file |
| `PADDOCK_TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | its password |
| `PADDOCK_TAURI_UPDATER_PUBKEY` | the contents of the public key file |
| `RELEASE_TOKEN` | optional, a user-level secret: a Gitea access token with `write:repository`; without it the release is published with the run's job token (`github.token`) |
| `APPLE_SIGNING_IDENTITY` | empty while the builds are ad-hoc sealed |

`dyno-release.yml` maps the `PADDOCK_TAURI_*` names to the `TAURI_*` names the shared `app-release.yml` expects. The bundle jobs
then build with `createUpdaterArtifacts` and the public key (passed as `tauri build --config`). **Without the key, a push,
nightly or manual run (forks, pull requests, a fresh repository) still builds unsigned installers without update support and
publishes no `latest.json`; a `dyno-v*` tag run fails** ("nothing is released"), so a release can never go out without
update support. The committed `tauri.conf.json` keeps `pubkey` empty, so local `npx tauri build` works without the private key.

**Back up the private key and its password** (password manager plus an offline copy). Never commit it. **If it is lost**, no
further update can be signed for the key built into installed copies: users install the next version by hand. **If it
leaks**, generate a new pair (`npx tauri signer generate`), publish a release signed with the new key, tell users to
reinstall from the releases page, and replace the secrets. Because the key is shared, this affects every Paddock app.

## What a release publishes

- Tag `dyno-vX.Y.Z` (the version in `tauri.conf.json`, `app/package.json` and the workspace `Cargo.toml` must all equal X.Y.Z),
  pushed from `main`; releases are made only by this workflow, never from a workstation.
- A Gitea release in `decryp7/paddock` with the installers, their `.sig` files, `RELEASE_NOTES.md` (the First launch block,
  then git-cliff over `apps/dyno/**` only, so the notes list Dyno's commits) and `latest.json`.
- The moving release `dyno-latest` carries the same `latest.json`; installed copies read
  `https://dev.decryptology.net/decryp7/paddock/releases/download/dyno-latest/latest.json`.
- The release is created as a draft and published only when every installer is signed and listed in `latest.json`.

## Before tagging

- `main` is green: the `plan` and `gates` jobs of `dyno-release.yml` (Dyno's legs of `ci.yml`) and the `bundle` jobs of the shared `app-release.yml`.
- `CHANGELOG`/release notes read well: `git cliff --latest` (Conventional Commits; no attribution trailers).
- The overhead numbers in `docs/architecture.md` were re-measured on a quiet machine for this release (roadmap G13).
- `docs/screenshots` match the built screens (`npm run screenshots` in `app/`).

## First launch note

Apple notarization and Windows code signing are deferred until after the releases settle (decision 2026-10-23). Until then the release workflow appends a "First launch" section to every release's notes (and so to `latest.json`) explaining **Open Anyway** (macOS) and **More info > Run anyway** (Windows SmartScreen).

## After the release: verify the installed app

Step 4 of `docs/blocks/releasing.md` installs the published build and runs `tools/verify-installed.sh --name Dyno` (macOS and
Linux: bundle, launch agent or systemd unit, socket, service answer, log tail). **Windows gap:** the script is bash only; there
is no Windows counterpart yet, so on Windows the installed app is checked by hand (start it, `dyno --version`, the tray icon)
until a PowerShell version is added.
