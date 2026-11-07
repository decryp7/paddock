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
| `prune-ci-build-folder.sh <folder> <max-gb> [--drop-bundles]` | the CI form: caps only a kept build folder of this job's slot (`job-isolation.sh owns <folder> target`: `target-<name>-<slot>` in the CI home, resolved, no symlinks), else leaves it alone | the `prune-build-folder` action |
| `prune-build-folders.ps1 -App debrief [-Remove -Keep 6c]` | list and remove build folders of finished worktrees (Windows) | Debrief `clean-build-dirs.ps1` |
| `host-cleanup.sh --id pitwall --process-pattern '{build}[/\\]debug[/\\]pitwall-server'` | this job's stray test processes (patterns anchored on its own folders; PowerShell on Windows) and temp folders on a host runner, then a leak check; used by the `host-cleanup` CI action | Pitwall `ci/macos-cleanup.sh` |

Parameters carry what differed between the copies: the product name, the launchd label, the systemd unit, the service
program, the data folder and socket name (defaults follow `paddock-env`), the launch-agent minimum macOS. What the verify
scripts read (unit, launchd state, discovery file) is what `paddock-service` (X4) registers; pass its names.

Dropped tool: [Scrutineer](scrutineer/README.md), code-signing certificates, was dropped on 2026-11-05; the apps stay unsigned. Its placeholder page is kept as a record.

Tests: `node --test tools/test/*.test.mjs`. They run every script in a scratch folder with a PATH of shims
(`PADDOCK_TOOLS_OS` pretends to be macOS or Linux), so no launchd, systemd, Docker or installed app is touched. The PowerShell
script is not run on the Mac; it is a parameterised port of Debrief's, reviewed by reading, and runs on the Windows runner.

## macos-vm-smoke.sh

Rule 7 forbids registering real launch agents or user units on a developer's or the shared runner's session, so those
smokes run in a throw-away VM: [Tart](https://tart.run) (Apple Virtualization.framework, Apple silicon, `brew install
cirruslabs/cli/tart`). Tart's licence is the Fair Source licence: free on personal computers and for organisations up to
100 CPU cores of Tart hosts (the repository's LICENSE file is FSL-1.1-ALv2, "internal use" permitted). Images are pinned by
digest in the script (`macos-tahoe-base`, `ubuntu` 24.04; the Cirrus images log in as `admin`/`admin` and carry the guest
agent `tart exec` needs). A run: a host-wide VM slot (`./dev ci slot` under `TART_HOME`, an OS lock, waited for; `PADDOCK_VM_MAX` 1 or 2), at least 60 GB free under `TART_HOME`, wait while the 1-minute
load is above 16, pull, clone, `tart run --no-graphics` with the staged builds shared read-only, `tart exec` the smoke,
then stop and delete the VM from a trap (success, failure, Ctrl-C, timeout). It cannot cover x86_64-only issues (the guests
are arm64) or Windows (Tart has no Windows guests); the Windows service path stays on the Windows runner.
`--guest linux` gives a real `systemd --user` session (`scripts/shell-demo/smoke-service-update-linux.sh`); a tray or
notification smoke on a desktop session is not written yet.

## paddock-dev (`./dev`)

`tools/paddock-dev/` is the developer and CI tool of ADR 0022: a Rust binary with its own Cargo workspace and lockfile
(ADR 0014's third kind), run through `./dev` (`dev.cmd` on Windows) at the repository root, from any subfolder. The wrapper
names the binary by an input hash (git's hash over the tool's inputs in `tools/paddock-dev/inputs.txt` plus the compiler
cargo will use), uses the per-user cache `~/.paddock-dev/bin/<hash>/` (`PADDOCK_DEV_HOME`), and builds into
`tools/paddock-dev/target/` (`PADDOCK_DEV_TARGET_DIR`; agents put it outside `~`) on a miss or when an input has
uncommitted changes. `PADDOCK_DEV_FRESH=1` builds from source in a fresh folder without a compiler wrapper (release jobs).
The prebuilt download (the `dev-tool.yml` package) is not wired yet. Commands today: `self hash`, `ci slot
acquire|release|status|idle-run`, `graph affected`; the rest arrives with R1 and later. When a path dependency in `crates/` changes its
dependencies, regenerate the tool's lockfile in the same change (`cd tools/paddock-dev && cargo update -w`).
