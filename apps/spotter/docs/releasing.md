# Releasing Spotter (and signed updates)

A `spotter-v*` tag on `main` runs `.gitea/workflows/spotter-release.yml` (repo root): Spotter's legs of `ci.yml` gate the tag
(Rust on Linux, Windows and macOS, the frontend with the Chromium and WebKit e2e), then the shared `app-release.yml` builds the
installers (Windows NSIS, the macOS `.dmg` sealed ad hoc and verified, Linux `.deb` and AppImage) and publishes a Gitea release
with the installers, release notes (`RELEASE_NOTES.md`, from git-cliff) and `latest.json`, the release information Spotter's
updater reads. It also puts `latest.json` on the moving `spotter-latest` release, which installed copies read. Installers are
also built for pushes to `main` that reach Spotter, the nightly run and manual runs (`workflow_dispatch`, no release; `gates: true`
also runs Spotter's `ci.yml` legs first).

```sh
git tag spotter-v1.0.0 && git push origin spotter-v1.0.0
```

The version in the tag must match `app/src-tauri/tauri.conf.json` and the workspace `Cargo.toml`. Pre-releases are
not offered to users: updaters read `latest.json` of the *latest* release.

This is about **Spotter's own** updates, not the releases Spotter watches for.

## How Spotter's updates work

- **Opt-in.** Spotter never checks on its own unless the user turns on automatic update checks (off by default); a
  manual check always works. The check and the install are built: Settings > Spotter updates shows the version, "Check for
  updates", the release notes and the install; a build without the public key says so and never checks.
- **One endpoint.** A check is one HTTPS GET of
  `https://dev.decryptology.net/decryp7/paddock/releases/download/spotter-latest/latest.json` (the moving `spotter-latest` release of the Paddock monorepo; copies installed before the move are reinstalled once by hand)
  (`plugins.updater.endpoints` in `tauri.conf.json`). It carries no identifier or usage data.
- **Signatures are mandatory.** Every update is verified with the minisign public key built into the app before it
  is installed, and `requireSignedVersion` binds each signature to its version (GHSA-j38x-g3m3-95fr). A build without
  the public key says "Updates are not available in this build" and never checks.
- `latest.json` is written by the shared `conformance/release/latest-json.mjs` from the signed installers (`*.sig` next to
  each); `node --test conformance/test/latest-json.test.mjs` (repo root) tests it.

## Signing key and secrets

Spotter uses the **family's shared updater key** (one minisign key pair for every Paddock app), kept as secrets of the
`decryp7/paddock` repository (Settings > Actions > Secrets), not as per-app secrets:

| Secret | Value |
| --- | --- |
| `PADDOCK_TAURI_SIGNING_PRIVATE_KEY` | the contents of the private key file |
| `PADDOCK_TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | its password |
| `PADDOCK_TAURI_UPDATER_PUBKEY` | the contents of the public key file |
| `RELEASE_TOKEN` | optional: a Gitea access token with `write:repository`; without it the release is published with the run's `github.token` |
| `APPLE_SIGNING_IDENTITY` | empty while the builds are ad-hoc sealed (see below) |

`spotter-release.yml` maps the `PADDOCK_TAURI_*` names to the `TAURI_*` names the shared `app-release.yml` expects. The bundle jobs
then build with `createUpdaterArtifacts` and the public key (passed as `tauri build --config`, and as `TAURI_UPDATER_PUBKEY`
for `option_env!` in `app/src-tauri/src/updater.rs`). Without the key (forks, pull requests) the installers are built without
update support and no `latest.json` is published.

The committed `tauri.conf.json` keeps `pubkey` empty and does not set `createUpdaterArtifacts`, so a local `npx tauri build`
works without the private key.

**Back up the private key and its password** (password manager plus an offline copy). Never commit it. **If it is lost**, no
further update can be signed for the key built into installed copies: users install the next version by hand. **If it
leaks**, generate a new pair (`npx tauri signer generate`), publish a release signed with the new key, tell users to reinstall
from the releases page, and replace the secrets. Because the key is shared, this affects every Paddock app.

## What a release publishes

- Tag `spotter-vX.Y.Z` (the version in `tauri.conf.json`, `app/package.json` and the workspace `Cargo.toml` must all equal X.Y.Z),
  pushed from `main`; releases are made only by this workflow, never from a workstation.
- A Gitea release in `decryp7/paddock` with the installers, their `.sig` files, `RELEASE_NOTES.md` (git-cliff over
  `apps/spotter/**` only, so the notes list Spotter's commits) and `latest.json`.
- The moving release `spotter-latest` carries the same `latest.json`; installed copies read
  `https://dev.decryptology.net/decryp7/paddock/releases/download/spotter-latest/latest.json`.
- `THIRD_PARTY_NOTICES.md` (Rust crates and production npm packages with their licence texts) is bundled as a resource.
  Regenerate it with `apps/spotter/scripts/notices/generate.sh` after a dependency change; the `licences` job of
  `spotter-release.yml` runs `cargo deny` and fails when the committed file is stale.

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

## First launch note

The macOS app is ad-hoc signed and **not notarised**, and the Windows installer is not code-signed: Apple notarisation and Windows code signing are deferred until after the releases settle (decision 2026-10-23). Until then the release workflow appends a "First launch" section to every release's notes (and so to `latest.json`) explaining **Open Anyway** (macOS) and **More info > Run anyway** (Windows SmartScreen).
