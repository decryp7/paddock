# paddock/tools

Dev and host scripts the apps copied five times, with the app as a parameter (plan X47). Run them from the app repository
(`bash paddock/tools/<script> ...`, with Paddock checked out beside it or fetched by the CI action); each one fails fast on
a bad option and never touches anything outside what it names.

| Script | What | Replaces |
|---|---|---|
| `bootstrap.sh --name Pitcrew [--check]` | check the prerequisites (macOS or Linux), `npm ci`, build the installers, print where they are and the next steps | Pitcrew `macos-bootstrap.sh`, `linux-bootstrap.sh`; Pitwall `linux-bootstrap.sh` |
| `verify-installed.sh --name Pitcrew --label ... --unit ... --service ...` | read-only check of an installed app: bundle, launchd agent or systemd user unit, socket, service answer, log tail | Pitcrew/Pitwall `macos-verify.sh`, `linux-verify.sh`; Dyno `linux-verify.sh` |
| `linux-container.sh --id dyno --packages dyno-platform` | cargo test and clippy in a Linux container; labelled containers and volumes only; `--clean` | Dyno `linux-container.sh`; Pitwall `linux-container-verify.sh` |
| `vm-smoke.sh macos\|linux` | build N and N+1 of the shell demo and run the update smoke in a local VM with one command (before a release; CI has no VMs) | the service-smoke workflow |
| `macos-vm-smoke.sh --old <N> --new <N+1> [--guest macos\|linux]` | run a smoke that registers real services (launch agent, systemd user unit) in a fresh, always-deleted Tart VM from a pinned image; disk, load and one-VM-at-a-time checks | the C21 service-after-update smoke (rule 7) |
| `prune-build-folders.sh <folder> <max-gb>` | keep a persistent CI build folder under a cap (incremental caches first, then the folder) | Dyno `ci-prune.sh` |
| `prune-ci-build-folder.sh <folder> <max-gb> [--drop-bundles]` | the CI form: caps only a `target-*` folder directly in `${PADDOCK_CI_HOME:-~/.paddock-ci}` (resolved, no symlinks), else leaves it alone | the `prune-build-folder` action |
| `prune-build-folders.ps1 -App debrief [-Remove -Keep 6c]` | list and remove build folders of finished worktrees (Windows) | Debrief `clean-build-dirs.ps1` |
| `host-cleanup.sh --id pitwall --process pitwall-server` | stray test processes and temp folders on a host runner; used by the `host-cleanup` CI action | Pitwall `ci/macos-cleanup.sh` |

Parameters carry what differed between the copies: the product name, the launchd label, the systemd unit, the service
program, the data folder and socket name (defaults follow `paddock-env`), the launch-agent minimum macOS. What the verify
scripts read (unit, launchd state, discovery file) is what `paddock-service` (X4) registers; pass its names.

Tests: `node --test tools/test/*.test.mjs`. They run every script in a scratch folder with a PATH of shims
(`PADDOCK_TOOLS_OS` pretends to be macOS or Linux), so no launchd, systemd, Docker or installed app is touched. The PowerShell
script is not run on the Mac; it is a parameterised port of Debrief's, reviewed by reading, and runs on the Windows runner.

## macos-vm-smoke.sh

Rule 7 forbids registering real launch agents or user units on a developer's or the shared runner's session, so those
smokes run in a throw-away VM: [Tart](https://tart.run) (Apple Virtualization.framework, Apple silicon, `brew install
cirruslabs/cli/tart`). Tart's licence is the Fair Source licence: free on personal computers and for organisations up to
100 CPU cores of Tart hosts (the repository's LICENSE file is FSL-1.1-ALv2, "internal use" permitted). Images are pinned by
digest in the script (`macos-tahoe-base`, `ubuntu` 24.04; the Cirrus images log in as `admin`/`admin` and carry the guest
agent `tart exec` needs). A run: lock (one VM at a time), at least 60 GB free under `TART_HOME`, wait while the 1-minute
load is above 16, pull, clone, `tart run --no-graphics` with the staged builds shared read-only, `tart exec` the smoke,
then stop and delete the VM from a trap (success, failure, Ctrl-C, timeout). It cannot cover x86_64-only issues (the guests
are arm64) or Windows (Tart has no Windows guests); the Windows service path stays on the Windows runner.
`--guest linux` gives a real `systemd --user` session (`scripts/shell-demo/smoke-service-update-linux.sh`); a tray or
notification smoke on a desktop session is not written yet.

## vm-smoke.sh

`bash tools/vm-smoke.sh macos|linux` is the one local entry point for tests that need a throwaway OS (login tests, services, installs, updates, system settings; the candidates are listed in the release checklist). Its first case is the update smoke (C21): it builds the shell demo as N and
N+1 (`scripts/shell-demo/build-macos-n-n1.sh`, `build-linux-arm64.sh`) and runs `macos-vm-smoke.sh` on them. VM storage
defaults to `TART_HOME=/Volumes/d7sst7/ci-tart`; it fails fast when Tart, Docker (linux) or the disk is missing. CI has no
VMs (user decision, 2026-10-28), so this is run by hand before a release, see the pre-tag step in [docs/blocks/releasing.md](../docs/blocks/releasing.md) and rule 25.

### Tests that need a throwaway OS

Local VMs are for tests that would otherwise change the machine they run on (rule 7): login tests (Login Items, background
services, launch at login) and installs, updates and system settings. `vm-smoke.sh` is the one entry point; the update smoke
is its first case. Candidates noted, not moved yet (they are faked or covered by fakes on the host today):

- `apps/dyno`: `app/src-tauri/src/login.rs`, `service_repair.rs`, `crates/dyno-agent/src/service/tests.rs`,
  `crates/dyno-platform/src/linux/startup.rs`, `crates/dyno-helper/src/install.rs` (launch agent, helper install, autostart entries).
- `apps/pitwall`: `app/src-tauri/src/service_install.rs`, `service_repair.rs`, `crates/pitwall-server/src/service/mod.rs`.
- `apps/debrief`: `crates/debrief-agent/src/service/launchd.rs`, `cli.rs` (agent install and start at login).
- `crates/paddock-service`: `scenarios.rs` runs over fakes; real launchd and systemd behaviour is only the update smoke.
