# paddock/tools

Dev and host scripts the apps copied five times, with the app as a parameter (plan X47). Run them from the app repository
(`bash paddock/tools/<script> ...`, with Paddock checked out beside it or fetched by the CI action); each one fails fast on
a bad option and never touches anything outside what it names.

| Script | What | Replaces |
|---|---|---|
| `bootstrap.sh --name Pitcrew [--check]` | check the prerequisites (macOS or Linux), `npm ci`, build the installers, print where they are and the next steps | Pitcrew `macos-bootstrap.sh`, `linux-bootstrap.sh`; Pitwall `linux-bootstrap.sh` |
| `verify-installed.sh --name Pitcrew --label ... --unit ... --service ...` | read-only check of an installed app: bundle, launchd agent or systemd user unit, socket, service answer, log tail | Pitcrew/Pitwall `macos-verify.sh`, `linux-verify.sh`; Dyno `linux-verify.sh` |
| `linux-container.sh --id dyno --packages dyno-platform` | cargo test and clippy in a Linux container; labelled containers and volumes only; `--clean` | Dyno `linux-container.sh`; Pitwall `linux-container-verify.sh` |
| `macos-vm-smoke.sh --old <N> --new <N+1> [--guest macos\|linux]` | run a smoke that registers real services (launch agent, systemd user unit) in a fresh, always-deleted Tart VM from a pinned image; disk, load and one-VM-at-a-time checks | the C21 service-after-update smoke (rule 7) |
| `prune-build-folders.sh <folder> <max-gb>` | keep a persistent CI build folder under a cap (incremental caches first, then the folder) | Dyno `ci-prune.sh` |
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
