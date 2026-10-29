# Releasing Dyno (and signed updates)

A `dyno-v*` tag on `main` runs `.gitea/workflows/dyno-release.yml` (repo root): first the gates (Dyno's legs of `ci.yml`, WebKit e2e included), then the shared `app-release.yml`, whose `bundle` jobs build the installers (Windows NSIS on the
`windows-latest` host runner, the macOS `.dmg` on the `macos-latest` host runner, and a `.deb` plus AppImage on a Linux
container), and the `release` job publishes a Gitea release with the installers, release notes (`RELEASE_NOTES.md`, from
git-cliff, or from the CHANGELOG for the first release) and `latest.json`, the release information Dyno's updater reads. Installers are also built for the nightly run
of `main` and manual runs (`workflow_dispatch`), and are uploaded as workflow artifacts (the v3 artifact actions, which
Gitea Actions supports).

See *Cutting the release* below for the tag (`dyno-v1.0.0` for the first release).

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
  `https://dev.decryptology.net/decryp7/paddock/releases/download/dyno-latest/latest.json` (the moving `dyno-latest` release of the Paddock monorepo) (`plugins.updater.endpoints` in
  `tauri.conf.json`). It carries no identifier or usage data.
- **Signatures are mandatory.** Every update is verified with the minisign public key built into the app before it is
  installed. A build without the public key says "Updates are not available in this build" and never checks.
- `latest.json` is written by the shared `publish-release` action (`conformance/release/`) from the signed installers (`*.sig` next to each); its tests are in `conformance/`.

## Signing: the family updater key

Dyno signs its updates with the **Paddock family key**, not a key of its own. These secrets live once, in the **paddock**
repository (`decryp7/paddock`, Settings > Actions > Secrets), and `dyno-release.yml` maps them to the names the shared
`app-release.yml` reads:

| Secret in `decryp7/paddock` | Passed as | Value |
| --- | --- | --- |
| `PADDOCK_TAURI_SIGNING_PRIVATE_KEY` | `TAURI_SIGNING_PRIVATE_KEY` | the family private key |
| `PADDOCK_TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | its password |
| `PADDOCK_TAURI_UPDATER_PUBKEY` | `TAURI_UPDATER_PUBKEY` | the matching public key |
| `RELEASE_TOKEN` | `RELEASE_TOKEN` | a Gitea access token with `write:repository`, used to publish the release |
| `APPLE_SIGNING_IDENTITY` | `APPLE_SIGNING_IDENTITY` | only once there is a Developer ID; empty means ad-hoc |

With the three key secrets set, the bundle jobs build with `createUpdaterArtifacts` and the public key (passed as
`tauri build --config`). **Without them (forks, pull requests) every build still works**: installers are produced unsigned and
without update support, and the release job publishes no `latest.json`. The committed `tauri.conf.json` keeps `pubkey` empty,
so a local `npx tauri build` works without the private key.

The key is generated, backed up (password manager plus an offline copy) and rotated for the whole family, never per app and
never in this app's folder. It is never committed and never in a test VM
(testing uses throwaway keys, see [testing-manual.md](testing-manual.md#install-and-update-in-a-throwaway-vm)).

**If the private key is lost**, no further update can be signed for the key built into installed copies: users install the
next version by hand. **If it leaks**, generate a new pair, publish a release signed with the new key, tell users to
reinstall from the releases page, and revoke the old secrets.

## Cutting the release

1. Everything below under *Before tagging* is done, and `main` is green.
2. Bump the version in `app/src-tauri/tauri.conf.json` and the workspace `Cargo.toml` (and the lock files) in a normal change.
   Give the `CHANGELOG.md` section its date.
3. Tag the commit that carries them, from `main`, and push the tag:

   ```sh
   git tag dyno-v1.0.0 && git push origin dyno-v1.0.0
   ```

   Release **only from a tag, in CI** (ER-15): never build or upload release installers from a workstation. A local build
   is for trying and is never published. **Never tag a commit whose message has `[skip ci]`** (ER-16): the tag would
   have no CI run to build from.
4. Watch the `dyno-release` run: the gates, then the three bundles, then the release job. It publishes the Gitea release
   (installers, `.sig` files, release notes) and writes `latest.json` to the release and to the moving `dyno-latest`
   release that installed copies read.

**Release notes.** `app-release.yml` normally asks git-cliff for the commits since the previous `dyno-v*` tag under
`apps/dyno/**`, `crates/**` and `ui/**`. The first release has no earlier tag, and the shared paths reach back through the
whole monorepo (hundreds of commits that are not Dyno's), so `dyno-release.yml` sets `first-release-changelog:
apps/dyno/CHANGELOG.md`: while no other `dyno-v*` tag exists, the notes are that file's `## [<version>]` section, and the run
fails if the section is missing or empty. From `dyno-v1.0.1` on, git-cliff takes over.

## First launch note

Apple notarization and Windows code signing are deferred until after the releases settle (decision 2026-10-23). Until then the release workflow appends a "First launch" section to every release's notes (and so to `latest.json`) explaining **Open Anyway** (macOS) and **More info > Run anyway** (Windows SmartScreen).
