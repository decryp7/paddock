# Releasing Pitwall (and signed updates)

A `pitwall-v*` tag on `main` runs `.gitea/workflows/pitwall-release.yml` (repo root): Pitwall's legs of `ci.yml` gate the tag
(Rust on Linux, Windows and macOS, the frontend with the Chromium and WebKit e2e), then the shared `app-release.yml` builds
the installers (Windows NSIS, the macOS `.dmg` sealed ad hoc and verified, Linux `.deb` and AppImage) and publishes a Gitea
release in `decryp7/paddock` with the installers, their `.sig` files, release notes (`RELEASE_NOTES.md`, from git-cliff) and
`latest.json`, the release information Pitwall's updater reads. It also puts `latest.json` on the moving `pitwall-latest`
release, which installed copies read. Installers are also built for pushes to `main` that reach Pitwall, the nightly run and
manual runs (`workflow_dispatch`, no release; `gates: true` also runs Pitwall's `ci.yml` legs first).

```sh
git tag pitwall-v1.1.0 && git push origin pitwall-v1.1.0
```

The version in the tag must match `app/src-tauri/tauri.conf.json`, `app/package.json` and the workspace `Cargo.toml`
(`check-tag.mjs` fails the run before the bundles are built). Updaters read the `latest.json` of the `pitwall-latest`
release; pre-releases are not offered.

**Release notes and changelog.** The notes of a release are the git-cliff changelog (`apps/pitwall/cliff.toml`, tags
`pitwall-v*`) over `apps/pitwall/**` and the shared crates Pitwall builds on (`crates/paddock-service`, `crates/paddock-jobs`, `crates/paddock-proc`;
`ui/**` joins after the shell move), so they list Pitwall's own commits. The changelog is the Releases page of the repository
(`https://dev.decryptology.net/decryp7/paddock/releases`, filter `pitwall-v`); there is no committed `CHANGELOG.md` for the
app, and `git cliff --config apps/pitwall/cliff.toml --include-path 'apps/pitwall/**' -o /dev/stdout` prints the whole history.

**Pitwall 1.0.x copies reinstall once, by hand.** Releases up to 1.0.x were published from `decryp7/pitwall` and are signed
with a different key, and their updater reads a different feed. Install 1.1.0 from the Releases page over the old copy
(settings, models and keys are kept; the background service is re-registered once, see the
[testing manual](testing-manual.md#update)); from then on the app updates itself from `pitwall-latest`.

## Signing key and secrets

Pitwall uses the **family's shared updater key** (one minisign key pair for every Paddock app), kept as secrets of the
`decryp7/paddock` repository (Settings > Actions > Secrets), not as per-app secrets:

| Secret | Value |
| --- | --- |
| `PADDOCK_TAURI_SIGNING_PRIVATE_KEY` | the contents of the private key file |
| `PADDOCK_TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | its password |
| `PADDOCK_TAURI_UPDATER_PUBKEY` | the contents of the public key file |
| `RELEASE_TOKEN` | optional: a Gitea access token with `write:repository`; without it the release is published with the run's `github.token` |
| `APPLE_SIGNING_IDENTITY` | empty while the builds are ad-hoc sealed (see below) |

`pitwall-release.yml` maps the `PADDOCK_TAURI_*` names to the `TAURI_*` names the shared `app-release.yml` expects. The bundle
jobs then build with `createUpdaterArtifacts` and the public key (passed as `tauri build --config`, and as
`TAURI_UPDATER_PUBKEY` for `option_env!` in `app/src-tauri/src/updater.rs`), and `conformance/release/latest-json.mjs` writes
`latest.json` from the signed installers (`node --test conformance/test/latest-json.test.mjs` at the repo root tests it).
Without the key (forks, pull requests) the installers are built without updater artifacts and without update support; the
app reports that updates are not available in this build and never checks, and no `latest.json` is published.

The committed `tauri.conf.json` keeps `pubkey` empty and does not set `createUpdaterArtifacts`, so a local
`npx tauri build` works without the private key.

**Back up the private key and its password** (password manager plus an offline copy). Never commit it. **If it is lost**, no
further update can be signed for the key built into installed copies: users install the next version by hand. **If it
leaks**, generate a new pair (`npx tauri signer generate`), publish a release signed with the new key, tell users to
reinstall from the releases page and replace the secrets. Because the key is shared, this affects every Paddock app.

## Unsigned builds

- macOS builds are sealed ad hoc (`signingIdentity: "-"`) because launch-agent registration through `SMAppService`
  refuses a linker-signed bundle (error -67056, see [suite-lessons.md](suite-lessons.md)). Ad hoc is not a Developer ID:
  Gatekeeper asks the user to approve the app on first launch ([macos.md](macos.md)).
- While the build is ad hoc the **hardened runtime is off** (`hardenedRuntime: false` in `tauri.macos.conf.json`): ad-hoc
  code under the hardened runtime enforces library validation and cannot load a library signed by its vendor ("different
  Team IDs"). When the `APPLE_SIGNING_IDENTITY` secret names a real identity, the shared `tauri-bundle` action passes
  `{"bundle":{"macOS":{"hardenedRuntime":true}}}` (notarization needs it). The shared `macos-verify` action
  (`conformance/release/macos-verify-bundle.sh`) fails the build when an ad-hoc Mach-O has the hardened runtime on, when
  bundled Mach-Os disagree on their TeamIdentifier, or when a sidecar does not start from the built app
  (`pitwall-server --version`), on the `.app` and on the copy inside the DMG.
- Windows installers are not Authenticode-signed; SmartScreen warns on first run ([windows.md](windows.md)).
- Linux packages are not signed.

## How updates will work

- **Opt-in.** Pitwall never checks on its own unless the user turns it on; a manual check always works.
- **One endpoint.** A check is one HTTPS GET of the release's `latest.json`. It carries no identifier or usage data.
  (Like the engine and model downloads, this is the only kind of traffic Pitwall makes, and offline mode stops it;
  see [ADR 0001](adr/0001-local-only.md). The update host has to be on the download allow-list before this ships.)
- **Signatures are mandatory.** Every update is verified with the public key built into the app before it is installed.
- **The server follows the app.** After an update the service restarts itself when its own binary changed, but only
  when idle (no requests, queued jobs or downloads), and only when something supervises it, so launchd, systemd or the
  Windows service manager starts the new binary (`service::spec::should_restart`).
- **Engines update separately.** The pinned llama.cpp release is part of the Pitwall source; a new pin ships with a new
  Pitwall release, and each build is checksummed and self-tested at install time ([ADR 0004](adr/0004-engine-abstraction-and-process-model.md)).

## Release checklist

1. `main` is green on all three platforms in CI.
2. When the pinned llama.cpp release changed (`crates/pitwall-core/src/runtimes/llama-runtimes.json`), verify it with the
   manual flow.
3. Bump the version in `Cargo.toml`, `app/package.json` and `app/src-tauri/tauri.conf.json` (they must agree).
4. Before tagging (rule 25): `cargo deny check advisories`, one CI build with bundles (the workflow's manual run), the icon
   check, signing secrets stripped of whitespace, and the release notes' "First launch" section.
5. Run the [testing manual](testing-manual.md) smoke test on every platform you can, including the real-model test and the
   [Update](testing-manual.md#update) section on the local VMs.
6. Tag `pitwall-vX.Y.Z` (never a `[skip ci]` commit) and push the tag from `main`; check the release page for the
   installers, `RELEASE_NOTES.md` and `latest.json`, and that `pitwall-latest` carries the same `latest.json`.
7. Install the published build (not the CI artifact) over the previous version on a real machine and confirm the server
   restarts into the new binary.
8. Before the tag (rule 54): a `pitwall-release` dispatch with `gates: true` whose three `bundle (...)` jobs passed and whose verdict job says `installers: built`; then `source ~/.zshenv; scripts/vm-smoke/vm-smoke.sh --app pitwall --run <that run id>` passes on a fresh clone, and its result file is attached (see [scripts/vm-smoke/README.md](../../../scripts/vm-smoke/README.md)).

## Verification status

Installing and running on Windows and Linux, and CUDA and Vulkan engine builds, have not been verified on real
machines yet; say so in the release notes of any early build. See [roadmap.md](roadmap.md).

<!-- paddock:block releasing rev=8ad0c1a8 -->
## Releasing from a tag

Releases are made by CI from the tag `pitwall-vX.Y.Z`, never from a workstation (rule 9). A local build for you to try is
fine; it is never published.

1. Make sure `main` is green in CI and the version in `tauri.conf.json`, `Cargo.toml` and `package.json` agree. The release
   notes are generated from the commits since the previous `pitwall-v*` tag (git-cliff); there is no committed changelog
   file to update.
2. Before tagging (rule 25): `cargo deny check advisories`, one CI build with bundles (the workflow's manual run), the
   icon check, signing secrets stripped of whitespace, and the release notes' "First launch" section (until the apps are
   signed).
3. Tag the commit (`git tag pitwall-vX.Y.Z`, never a `[skip ci]` commit) and push the tag. The tag workflow runs the gates
   again, builds the installers on all platforms, verifies the macOS bundle (`codesign --verify --deep --strict` on the .app
   and the DMG), publishes the release with its notes and `latest.json`.
4. After the release: install the published build (not the CI artifact) and run the app's installed-app verify script.
<!-- /paddock:block releasing -->

<!-- paddock:block first-launch rev=4679a047 -->
### First launch

Pitwall is not code-signed yet, so your system may warn the first time you open it.

- **macOS:** if it says the app cannot be opened, choose Done, then System Settings > Privacy & Security > scroll to the message about Pitwall > **Open Anyway**.
- **Windows:** if SmartScreen says "Windows protected your PC", choose **More info > Run anyway**.
<!-- /paddock:block first-launch -->
