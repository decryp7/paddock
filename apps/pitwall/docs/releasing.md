# Releasing Pitwall

A `v*` tag runs the release workflow in `.gitea/workflows`: the bundle jobs build the installers on the three
platforms (Windows NSIS on the Windows runner, `.deb` and AppImage on Linux, `.dmg` on the macOS host runner), and
the release job publishes a Gitea release with the installers, release notes from git-cliff and `latest.json`, the
information Pitwall's updater reads. Installers are also built for nightly runs and manual runs of the workflow.

```sh
git tag v0.2.0 && git push origin v0.2.0
```

The version in the tag must match `app/src-tauri/tauri.conf.json` and the workspace `Cargo.toml`. Updaters read the
`latest.json` of the latest release; pre-releases are not offered.

Status today: the Tauri shell exists (`app/src-tauri`, with `externalBin` bundling the server as a sidecar and a
separate `tauri.<os>.conf.json` per platform), `.gitea/workflows/build.yml` checks every push on all three platforms and
`release.yml` builds and publishes for `v*` tags. Nothing has been released and no installer has been published yet, and
the installers have not been installed on real Windows and Linux machines. The server binary also builds on its own
(`cargo build -p pitwall-server --profile dist`).

## Unsigned builds

The signing secrets are **not set yet**. Builds must work without them, and do:

- Without `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_UPDATER_PUBKEY` the installers are built without updater artifacts and
  without update support; the app reports that updates are not available in this build and never checks. The release
  job publishes the installers but no `latest.json`.
- The committed `tauri.conf.json` keeps `pubkey` empty and does not set `createUpdaterArtifacts`, so a local
  `npx tauri build` works without any key.
- macOS builds are sealed ad hoc (`signingIdentity: "-"`) because launch-agent registration through `SMAppService`
  refuses a linker-signed bundle (error -67056, see [suite-lessons.md](suite-lessons.md)). Ad hoc is not a Developer ID:
  Gatekeeper asks the user to approve the app on first launch ([macos.md](macos.md)).
- While the build is ad hoc the **hardened runtime is off** (`hardenedRuntime: false` in `tauri.macos.conf.json`): ad-hoc
  code under the hardened runtime enforces library validation and cannot load a library signed by its vendor ("different
  Team IDs"). When the `APPLE_SIGNING_IDENTITY` secret names a real identity, `scripts/ci/tauri-bundle.sh` passes
  `{"bundle":{"macOS":{"hardenedRuntime":true}}}` (notarization needs it). `scripts/ci/macos-verify-bundle.sh` fails the
  build when an ad-hoc Mach-O has the hardened runtime on, when bundled Mach-Os disagree on their TeamIdentifier, or when a
  sidecar does not start from the built app (`--version`), on the `.app` and on the copy inside the DMG.
- Windows installers are not Authenticode-signed; SmartScreen warns on first run ([windows.md](windows.md)).
- Linux packages are not signed.

## The signing secrets (to set once)

Pitwall needs its **own** updater key pair; never reuse another product's key.

1. Generate a key pair on a trusted machine with a strong password:

   ```sh
   cd app
   npx tauri signer generate -w ~/.tauri/pitwall-updater.key
   ```

   This writes the encrypted private key `pitwall-updater.key` and the public key `pitwall-updater.key.pub`.

2. Set these repository secrets in Gitea (Settings, Actions, Secrets) of `decryp7/pitwall`:

   | Secret | Value |
   | --- | --- |
   | `TAURI_SIGNING_PRIVATE_KEY` | the contents of `pitwall-updater.key` |
   | `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | its password |
   | `TAURI_UPDATER_PUBKEY` | the contents of `pitwall-updater.key.pub` |
   | `RELEASE_TOKEN` | a Gitea access token with `write:repository` scope, used to publish the release |

   None of them is set yet. With them, `scripts/ci/tauri-bundle.sh` builds with `createUpdaterArtifacts` and the public
   key passed as a `--config` override, and `scripts/release/latest-json.mjs` writes `latest.json` from the signed
   installers (`node --test scripts/release/latest-json.test.mjs` tests it).

3. Back up the private key and its password (password manager plus an offline copy). Never commit it.

If the private key is lost, no further update can be signed for the key built into installed copies: users install the
next version by hand. If it leaks, generate a new pair, publish a release signed with the new key, tell users to
reinstall from the releases page and revoke the old secrets.

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
2. Update `CHANGELOG.md` (`git cliff -o CHANGELOG.md`) and, when it changed, the pinned llama.cpp release
   (`crates/pitwall-core/src/runtimes/llama-runtimes.json`), verified with the manual flow.
3. Bump the version in `Cargo.toml` and `app/src-tauri/tauri.conf.json`.
4. Run the [testing manual](testing-manual.md) smoke test on every platform you can, including the real-model test.
5. Tag `vX.Y.Z` and push the tag; check the release page for the installers (and `latest.json` once signing is set up).
6. Install over the previous version on a real machine and confirm the server restarts into the new binary.

## Verification status

Installing and running on Windows and Linux, and CUDA and Vulkan engine builds, have not been verified on real
machines yet; say so in the release notes of any early build. See [roadmap.md](roadmap.md).

## First launch note

Apple notarization and Windows code signing are deferred until after the releases settle (decision 2026-10-23). Until then the release workflow appends a "First launch" section to every release's notes (and so to `latest.json`) explaining **Open Anyway** (macOS) and **More info > Run anyway** (Windows SmartScreen).
