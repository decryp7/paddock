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

- **Windows:** `Dyno_<version>_x64-setup.exe` (NSIS, per-machine in Program Files since 1.1.0).
- **macOS:** `Dyno_<version>_aarch64.dmg`, and with signing secrets also the `.app.tar.gz` the updater installs. The bundle is
  **ad-hoc signed** (`signingIdentity "-"` in `tauri.macos.conf.json`): `SMAppService` and launch agents refuse a
  linker-signed bundle. It is not notarised: users open it once through Privacy & Security > Open Anyway (twice, then an administrator password; macOS 26). Developer ID signing and
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
folders (never a build directory), the macOS seal must verify, and `dyno-agent` must be present. **Installed-app check on macOS 27.x (Dyno ADR 0026), before the tag:** install the built `.dmg`, let the LaunchAgent start the agent, open the Network page and confirm it lists a socket owned by a program other than Dyno (and no "only its own sockets" message). `check-bundle.sh` proves the sidecars' code-signing identifiers (`net.decryptology.dyno.*`); only this proves the installed agent is shown the whole kernel list, which depends on how it was launched, and a shell run does not count.

Dyno 1.0 ships without its privileged helper: the `dyno-helper` crate stays in the workspace for 1.1 but is not built into or bundled with any installer, and no installer is expected to contain it.

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
| `RELEASE_TOKEN` | `RELEASE_TOKEN` | a Gitea access token with `write:repository`, used to publish the release. It is a **user-level secret of `decryp7`** (Settings > Actions > Secrets of the user), not a repository secret, so it does not appear in the paddock repository's secret list (`GET /api/v1/repos/decryp7/paddock/actions/secrets`) |
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

## Before tagging

In this order, on one commit:

0. Every 1.0 branch is merged: disk-map-1.0, jobs-1.0 (with its CHANGELOG bullet folded into the `[1.0.0]` section), tables/layout and the startup fix. Nothing may land after step 1 without repeating steps 2 and 3.
1. Bump the version (and, in the same commit, update the README status line, for example "1.1.0 released") in `app/src-tauri/tauri.conf.json` and the workspace `Cargo.toml` (and the lock files), and date the
   `CHANGELOG.md` section (`## [1.0.0] - <date>`, never "Unreleased"; the date is set in this bump commit to the tag date): the first release's notes are that section and the
   release run fails without it. `main` is green.
2. Run one **manual `dyno-release` with `gates: true`** on that commit (Actions > dyno-release > Run workflow, rule 25). It
   runs Dyno's legs of `ci.yml` (WebKit included) and builds the installers on all three systems.
   Then confirm the run's verdict job says `installers: built` (all three `bundle (...)` jobs passed): a green run that says
   `installers: not built` proves nothing about the release path (rule 54).
2b. Run the scripted VM start check on those installers, on a fresh clone: `source ~/.zshenv; scripts/vm-smoke/vm-smoke.sh --app dyno --run <the run id>`
   ([scripts/vm-smoke/README.md](../../../scripts/vm-smoke/README.md)). Attach its result file; a failed step blocks the tag.
2c. Run the benchmark unit check for the Linux runner: `scripts/linux-units-check.sh` (OrbStack `paddock-gate-arm`, a systemd user manager; it runs `cargo test -p dyno-agent --test bench_units -- --ignored --test-threads=1`, ADR 0030 D5). A failure blocks the tag: set `LINUX_UNITS_VERIFIED` to false in `crates/dyno-agent/src/bench.rs` or fix it.
2d. Release items that need hardware this repository's agents do not have, for the benchmark tool (ADR 0030): run `scripts/bench-smoke.sh` against each installed build (it starts both runners with tiny sizes and asserts mechanics only); verify direct I/O on a **Windows** and on a **Linux** runner (the disk test must report "Direct I/O: yes" on the system drive, or say why not, and leave no scratch file); and do the signing dry runs of `dyno-bench-gpu` on all three systems (macOS signing id and notarization, Windows signing, Linux package contents via `check-bundle.sh`). A failure blocks the tag.
3. Run the install and update plan, rows 24a to 24i of [testing-manual.md](testing-manual.md#install-and-update-in-a-throwaway-vm),
   on those installers (the VM plan says how the test builds are made).
3b. Run the power checks of [ADR 0032](adr/0032-power-keep-awake-wake-on-lan-idle-actions-energy.md) (P12) on those installers, in the
   release VM matrix and on real hardware. These three could not be verified with the test VMs, so they block the tag until done:
   - **Windows tray menu:** the Keep awake submenu (durations, display, While busy, Until a program quits), its header line, and the
     countdown entries ("Will sleep in 0:48", Cancel, Snooze 30 minutes) with the notification, in the real tray, with the Keep awake
     tray item on and off (Cancel must show with it off).
   - **Real sleep:** Sleep and Hibernate through the countdown on real hardware (the Windows VM has no sleep states, `powercfg /a`
     shows none): the computer sleeps and wakes, the idle rule re-arms only after new activity, and where a system cannot (hibernation
     off) the page says why. Windows `SetSuspendState`, macOS `pmset sleepnow`, Linux `systemctl suspend`.
   - **Real audio detection:** with a sound playing, `dyno idle` says audio is playing and the rule waits (Windows WASAPI sessions on
     the default playback device, macOS `coreaudiod`'s assertion, Linux "Playing audio" inhibitor); with it stopped the countdown
     starts. The Windows VM has no audio device, so only silence was seen there.
   Test VMs: revert the Windows VM to the `gui-session-tools` snapshot (not `clean-baseline`) and delete Tart clones afterwards.
4. Re-measure the overhead numbers in `docs/architecture.md` on a quiet machine (roadmap G13) and refresh `docs/screenshots`
   against the built screens (`npm run screenshots` in `app/`).
5. Check the notes: the `CHANGELOG.md` section reads well (it is the first release's notes). For a later release:
   `git cliff --latest --config apps/dyno/cliff.toml --include-path 'apps/dyno/**'` (Conventional Commits; no attribution trailers).
6. Tag **that same commit** (below). Any later commit means steps 2 and 3 are run again.

## Cutting the release

1. *Before tagging* is done on the commit to release.
2. Tag it, from `main`, and push the tag:

   ```sh
   git tag dyno-v1.0.0 && git push origin dyno-v1.0.0
   ```

   Release **only from a tag, in CI** (ER-15): never build or upload release installers from a workstation. A local build
   is for trying and is never published. **Never tag a commit whose message has `[skip ci]`** (ER-16): the tag would
   have no CI run to build from.
3. Watch the `dyno-release` run: the gates, then the three bundles, then the release job. It publishes the Gitea release
   (installers, `.sig` files, release notes) and writes `latest.json` to the release and to the moving `dyno-latest`
   release that installed copies read.

**Release notes.** `app-release.yml` normally asks git-cliff for the commits since the previous `dyno-v*` tag under
`apps/dyno/**`, `crates/**` and `ui/**`. The first release has no earlier tag, and the shared paths reach back through the
whole monorepo (hundreds of commits that are not Dyno's), so `dyno-release.yml` sets `first-release-changelog:
apps/dyno/CHANGELOG.md`: while no other `dyno-v*` tag exists, the notes are that file's `## [<version>]` section, and the run
fails (in the bundle job, before the three builds) if the section is missing, still says "Unreleased", or has nothing but blank lines and link references (`conformance/ci/first-release-notes.sh`, tested by `conformance/test/first-release-notes.test.mjs`). From `dyno-v1.0.1` on, git-cliff takes over.

## Windows Security false positives (`paddock-setup.exe`)

The apps stay unsigned (decision 2026-11-05, kept 2026-11-08). Windows Security's machine-learning model has flagged `paddock-setup.exe` once
(`Trojan:Win32/Bearfoos.A!ml`, one VM install in six of the 1.1.0 release candidate, 2026-11-08, with the signatures of 2026-11-02; the research note is not in the tree, so the facts used here are stated inline). A verdict is per file hash, so the program is built
to keep its bytes:

- **Its own version.** `paddock-setup` has the version of its crate (`crates/paddock-setup/Cargo.toml`), not the app's. `FileVersion` and the
  installer's `check-version` use it (read from the embedded file at compile time, include v5). Change the crate version only when the program's
  code or constants change; an app release alone leaves the hash as it was.
- **Deterministic link and paths.** `build.rs` links with `/Brepro` and `/DEBUG:NONE`; `scripts/stage-paddock-setup.mjs` remaps the home folder,
  cargo home and checkout with `--remap-path-prefix` for this one build. The gate (`check-per-machine-installer.mjs`) fails a program without the
  full version resource (CompanyName, FileDescription, FileVersion, InternalName, LegalCopyright, OriginalFilename, ProductName), without the
  `asInvoker` manifest with `supportedOS`, with a different `FileVersion` than the crate's, or with a build path in it.
- **What was measured (2026-11-08).** On a Mac with `cargo xwin` (rustc 1.99.1, cargo-xwin-xwin 0.23.1; the toolchain is the floating `stable` channel, so the hash changes
  whenever the toolchain does): the same commit built three times from clean target folders, with app versions 1.1.1, 1.2.0 and 1.3.0, gave the same
  sha256 (see the commit body of the change). One of the three was built from a second checkout path. **Not measured:** the Windows runner
  (rust-lld via plain cargo), and equality between that build and the Mac's. Do it before the first release that relies on it: build twice with
  `node scripts/stage-paddock-setup.mjs --app dyno` from two checkout folders and compare the printed `paddock-setup.exe sha256` lines; then fill
  `released-hashes.json`.

**After each tag** (once per release, from the tag's installer):

1. Read the `paddock-setup.exe sha256` line of the bundle job's log. If it equals the previous release's, the earlier verdict carries over: record
   that and stop here for `paddock-setup.exe`; still submit the installer (step 3).
2. Submit `paddock-setup.exe` (extract it from the installer with 7-Zip, check the sha256) at <https://www.microsoft.com/wdsi/filesubmission>
   as a **software developer**, choosing "Incorrectly detected as malware/malicious", with the detection name seen in the evidence, the file's
   sha256 and a one-line description ("per-machine installer helper of the Dyno desktop app, built from a public tag").
3. Submit the setup installer (`Dyno_<version>_x64-setup.exe`) the same way.
4. Add the sha256 of step 1 to `crates/paddock-setup/released-hashes.json` under the app's identifier and the program's version. The next release's gate
   reads the program version at the previous `dyno-v*` tag: the same version without a record **fails** ("not measured"), a new version only warns.
5. Record the submission ids, the date and Microsoft's determination in the release log (the release's engineering notes, as `release-1.1.0-engineering-notes.md` was) when
   it arrives (days, not hours). Also read VirusTotal's tags for both files ("obfuscated", "detect-debug-environment") and note them.
6. The VM harness (branch `tools/vm-smoke-defender`, which lands before this one) prints the full Defender evidence on every Windows install row
   and has `node scripts/vm-smoke/defender-rate.mjs --installer <setup.exe>` (10 installs, each from a reverted snapshot, with an EICAR control): run it on the final installer and keep the
   rate with the submission ids.

## First launch note

The apps are not signed or notarized: updates are protected by the family update key, and paid certificates would be needed to remove the OS warnings (decision 2026-11-05). The release workflow appends a "First launch" section to every release's notes (and so to `latest.json`) explaining **Open Anyway** (macOS) and **More info > Run anyway** (Windows SmartScreen).

## The helper's signature (macOS) and how to test it locally

The macOS helper that the one password prompt copies into `/Library/PrivilegedHelperTools` is verified by the elevated install
against a minisign signature made with the family update key (security review of the helper install; ADR 0027 amendment).

- `app/src-tauri/scripts/sidecar.mjs` signs the final, ad-hoc sealed helper under the file name `dyno-helper-<version>-<target
  triple>`; the signature's trusted comment carries that name, and the helper accepts only its own version and architecture. The
  signature is bundled as `Contents/Resources/dyno-helper.minisig`; the hash it covers goes to `binaries/dyno-helper.sha256`.
- `scripts/check-bundle.sh` fails the build when the helper in the .app is not the file that was signed, and fails a tag build (or
  a build with `TAURI_UPDATER_PUBKEY` set) whose helper is unsigned. Pull-request and dev builds only warn.

Testing without the family key: make a throwaway pair and build with it.

```sh
cargo tauri signer generate -w /tmp/throwaway.key -p ""           # prints the public key; keep both out of the repository
export TAURI_SIGNING_PRIVATE_KEY="$(cat /tmp/throwaway.key)" TAURI_SIGNING_PRIVATE_KEY_PASSWORD=""
export TAURI_UPDATER_PUBKEY="$(cat /tmp/throwaway.key.pub)"        # compiled into the helper by the same build
node apps/dyno/app/src-tauri/scripts/sidecar.mjs && (cd apps/dyno/app && npx tauri build -- --profile dist)
```

The key must be the same for the signature and for the build that compiles `TAURI_UPDATER_PUBKEY` in, or the install says the
helper does not match its signature. Unit tests use the fixtures in `crates/dyno-helper-service/tests/fixtures/` (a throwaway key).
