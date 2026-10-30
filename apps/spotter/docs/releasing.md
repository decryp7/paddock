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

## Before the tag: the test VMs

CI runs Spotter's real-browser tests (`spotter-render`'s `real_browser`) only in the Linux leg, under Xvfb: the macOS and
Windows runners run in a user's session, where a test never starts a browser (Paddock rule 7). Before every tag, run them
in the local macOS (Tart) and Windows test VMs, never on a desktop and never in CI (docs/dev-setup.md, "Test VMs and real
browsers", repo root):

```sh
PADDOCK_LIVE_BROWSER=vm SPOTTER_REQUIRE_TEST_BROWSER=1 cargo nextest run -p spotter-render -E 'binary(real_browser)'
```

with `SPOTTER_TEST_BROWSER` naming the pinned Chrome for Testing in the VM (`node scripts/install-test-browser.mjs`).
Release-notes checklist item: **VM smoke: <date> <version> pass** (macOS and Windows).

## How Spotter's updates work

- **Opt-in.** Spotter never checks on its own unless the user turns on automatic update checks (off by default); a
  manual check always works. Checking, downloading and installing are the shell's Updates window (Help ▸ Check for
  updates, and the About dialog): `paddock-updater` finds the update and downloads it through the app's downloader, and
  its installer, `PluginInstaller` over `tauri-plugin-updater` (registered in `run()`; `paddock-shell/install` is
  enabled in `app/src-tauri/Cargo.toml`), verifies the signature again and installs the platform's package. A build
  without an installer would find updates it can never install, so `updater_tests` in `lib.rs` checks the built app
  has one, and `plugins.updater` in `tauri.conf.json` keeps `installMode: passive` for the Windows installer.
- **One endpoint.** A check is one HTTPS GET of
  `https://dev.decryptology.net/decryp7/paddock/releases/download/spotter-latest/latest.json` (the moving `spotter-latest` release of the Paddock monorepo; copies installed before the move are reinstalled once by hand)
  (`plugins.updater.endpoints` in `tauri.conf.json`). It carries no identifier or usage data.
- **Signatures are mandatory.** Every update is verified with the minisign public key built into the app before it
  is installed, and `requireSignedVersion` binds each signature to its version (GHSA-j38x-g3m3-95fr). A build without
  the public key says "Updates are not available in this build" and never checks.
- `latest.json` is written by the shared `conformance/release/latest-json.mjs` from the signed installers (`*.sig` next to
  each); `node --test conformance/test/latest-json.test.mjs` (repo root) tests it.

## One-time set-up: the family's signing key

Spotter signs its updates with the **Paddock family key** (one key pair for every Paddock app, kept as repository secrets of
`decryp7/paddock`); it needs no key of its own. If the family key does not exist yet, generate it once on a trusted machine
with a strong password:

```sh
npx tauri signer generate -w ~/.tauri/paddock-updater.key
```

This writes the encrypted private key `paddock-updater.key` and the public key `paddock-updater.key.pub`.

1. The repository secrets (Settings ▸ Actions ▸ Secrets of `decryp7/paddock`) are the family's:

   | Secret | Value |
   | --- | --- |
   | `PADDOCK_TAURI_SIGNING_PRIVATE_KEY` | the contents of the private key file |
   | `PADDOCK_TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | its password |
   | `PADDOCK_TAURI_UPDATER_PUBKEY` | the contents of the public key file |

   Also `RELEASE_TOKEN`: a Gitea access token with `write:repository` scope, used to publish the release.

   `spotter-release.yml` maps them to the `TAURI_*` names of the shared `app-release.yml`. The bundle jobs then build with
   `createUpdaterArtifacts` and the public key (passed as `tauri build --config`, and as `TAURI_UPDATER_PUBKEY` for
   `option_env!` in `app/src-tauri/src/lib.rs`, read by the shell's updater). Without the secrets (forks, pull requests)
   the installers are built without update support and the release job publishes no `latest.json`.

   The committed `tauri.conf.json` keeps `pubkey` empty and does not set `createUpdaterArtifacts`, so local
   `npx tauri build` works without the private key.

2. **Back up the private key and its password** (password manager plus an offline copy). Never commit it, never put
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

## First launch note

Apple notarization and Windows code signing are deferred until after the releases settle (decision 2026-10-23). Until then the release workflow appends a "First launch" section to every release's notes (and so to `latest.json`) explaining **Open Anyway** (macOS) and **More info > Run anyway** (Windows SmartScreen).

## Before tagging

The branch gates are the affected ones; the tag runs everything (see above). Before a first release, also by hand on one
machine per OS: install the previous build, publish a newer one to a test feed, and check that Updates finds it **and installs
it** (the installer is only reached from the real window), then that Spotter starts again on the new version.

## Pre-release update test (local VMs, results)

Manual, local only (never CI, never published): the installed copy must find a newer signed build, download it, verify the
signature, install it and relaunch (`PluginInstaller`, paddock-updater's `install` feature). Builds run on the host with a
test updater key made for the run (never the family key); the feed is a small HTTPS server on a host-only address, reached
through a throw-away test CA that only the VM trusts. The feed URL is the `UPDATE_FEED` constant in `lib.rs`; there is no
build-time override yet, so the test builds change that one string (and the version) in a scratch worktree, uncommitted.

### 2026-10-29, 29d65b6bd (`spotter/shell-1.0`), builds 1.0.0-test.1 and 1.0.0-test.2

| OS | Result |
| --- | --- |
| macOS 26.6.2 (25G83) guest, Tart `macos-tahoe-base`, host macOS 27.2 | **FAIL, blocked at step 2**: the installed test.1 never opens a window |
| Windows 11 Arm (Fusion, the user's VM) | **NOT RUN**: the VM is encrypted (`vmrun`: "A password is required"), so it cannot be started headless; no agent user or OpenSSH was verifiable |

macOS steps: both builds (aarch64, `--profile dist`, `.app.tar.gz` plus `.sig` with the test key) built; a test.2 signed with
a second key was built for the negative case; test.1 installed in a fresh VM clone (CA trusted, feed reachable by `curl`
from the guest, version read from `Info.plist`: 1.0.0-test.1). Launch (`open` and the binary directly, with and without a
fresh data folder, 3 of 3 runs): the process stays up with a status item but no window is ever created (System Events lists no
windows), nothing reaches `app.log` or stderr, and `sample` shows the main thread and `paddock-job-1` both blocked in
`pthread_mutex` waits after start-up (binary stripped, so no symbols). Steps 3 to 7 (Updates, download, verify, install,
relaunch, Install Chromium progress and Cancel, wrong-key refusal) were therefore not run. Not yet known whether the hang is
specific to the VM (no GPU, paravirtual Metal) or a lock-order bug; the same build must be started once on a real Mac or
an unstripped build sampled. Evidence: `/Volumes/d7sst7/dev-targets/handoff/spotter-update-test-2026-10-29/`.

The hang was the start-up deadlock of ADR 0029 (a window built inside a plugin hook), fixed before the 1.0.0 release; the next VM run (runbook: `spotter-1.0-vm-runbook.md`) is the first on the fix.
