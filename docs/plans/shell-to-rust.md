# Every shell script to Rust: inventory, exceptions and work packages

- Status: plan, decided. The user decided on 2026-11-15: "move all shell now", before the app releases, and took D1 to D9 the
  same day (section 8; D5 changed: the PowerShell logic is ported too). Running lanes: WP-0 (`feat/no-new-shell-check`), WP-1
  (`feat/dev-launch-rust`), WP-2 (`feat/retire-ci-kit-shell`), WP-7 (`feat/port-dev-app-scripts`). WP-5 and WP-6 start after
  train 12 lands.
- Date: 2026-11-15.
- Heads read (ADR 0091), re-read right before committing: `origin/main` at `23213a35e`, `origin/train/2026-11-15-12` at
  `285c8f47a`, `origin/train/2026-11-15-13` at `13881087a`, `origin/fix/c125-and-arm64-tests` at `f2e644a9f`,
  `origin/fix/stale-disk-paths` at `87bca2bf5`, `origin/feat/test-env-builder` at `7edb8cad6`,
  `origin/fix/hooks-ci-and-config-guard` at `bbee4708a`, `origin/tools/dev-fetch-r3` at `23e642f00`, `origin/tools/dev-notices`
  at `dcab304eb`, `origin/tools/vm-smoke-unified` at `892b2c29c`. Every count below is measured on `origin/main` unless a branch
  is named.
- Related: [ADR 0022](../adr/0022-rust-developer-and-ci-tool.md) (one Rust tool; §3.2 the launchers, §3.3 the content-addressed
  binary, §3.7 the shrink-only check, §4 what stays non-Rust), [ci-in-rust.md](ci-in-rust.md) (the CI port, 2026-11-12; this
  plan takes over its shell parts and moves them before the tags), ADR 0083 (the guard hook), ER-22, ER-37, ER-40, ER-41, ER-42,
  rule 54, rule 55.
- Estimates are raw agent-days for one implementer, with one review fix round per WP. They are estimates, not measurements.

## Why now

Train 12's CI run 8138 (ci.yml on `train/2026-11-15-12` at `285c8f47a`, conclusion `failure`, checked through the Gitea API) had
failed jobs `paddock-dev (windows-latest)` (39335), `paddock-dev (ubuntu-latest)` (39334), `tools and shared scripts` (39301),
`VM tooling tests (macOS)` (39302) and `ui` (39294). Three of its causes were shell:

| Failure | Cause | Fix commit (branch) |
|---|---|---|
| 11 `paddock-dev` tests on Windows (job 39335) | `./dev` and the gate scripts are bash and call `grep`, `tr` and `touch`; the tests' curated PATH had no Git `usr/bin` | `7edb8cad6` (`feat/test-env-builder`) |
| the prune parity test on Linux (job 39334) | `tools/prune-build-folders.sh` and `ci_host/prune.rs` counted folder blocks, which ext4 gives 4 KiB each. Parity held; both were wrong | `ffe91b1ac` (`fix/c125-and-arm64-tests`) |
| `tools` and `ui` jobs, VM tooling tests | `scripts/vm-win/target.sh` gave `./dev` a fixed PATH that hid CI's cargo | `bbee4708a` (`fix/hooks-ci-and-config-guard`) |

The same branch had two earlier fixes of the same class in the kit's shell: `a68ef939d` (GNU `du --apparent-size` counts folders
as 0, BSD `du -A` does not) and `9e2144e9c` (GNU `stat -f` printed file-system lines that summed to 1.2e10 and removed a kept
folder; review P1-1). On 2026-11-15 the same branch changed `kit.sh` and the prune script again (`f2e644a9f`), so the shell copy
still costs work while the Rust port exists. The class is old. ADR 0022's context says most past kit fixes were Git Bash and
bash 3.2 differences. ci-in-rust.md §8 lists a bash octal read of `010` and the guardian parity that needs bash 4.4.

## 1. Inventory

Measured on `origin/main` at `23213a35e`: 116 `*.sh` files (8,151 lines, of which 8 `.test.sh` files are 685) and the `dev`
wrapper (639), so 117 files and 8,790 lines. Also counted here, because they are shell too: 16 `*.ps1` files (1,489 lines),
`dev.cmd` (10), the hook shim `tools/githooks/claude-pre-tool-use` (7), Dyno's three Debian maintainer scripts, and 6 Python
files under `.claude/hooks` (1,690 lines) that the `.claude/hooks/*.sh` files only `exec`. Inline workflow shell: 328 `run:`
blocks with 947 non-comment lines in `.gitea/workflows`, and 43 blocks with 361 lines in `.gitea/actions` (one script over the
YAML; not in this plan's scope, see D7).

"Lines" are `wc -l` (comments included). "Called by" comes from `git grep` of the path and file name over `origin/main`:
**CI** names the workflow or action, **kit** means through `conformance/ci/kit.sh`'s shell route (tag, release and rule 54 runs
only; other runs take the Rust port), **pre-push** is `tools/paddock-dev/src/pre_push`, **docs** means only documents name it,
**test** means only a test runs it. OS: M macOS, L Linux, W Windows (Git Bash), and "guest" for a VM's own OS.

### 1.1 The launcher and the bootstrap

| Script | Lines | Called by | OS | Rust home | WP |
|---|---:|---|---|---|---|
| `dev` | 639 | every `./dev` call: actions, workflows, lefthook, scripts, tests, people | M L W | `tools/dev-launch` (new, std only) behind a stage-0 `dev` of about 20 lines | WP-1 |
| `dev.cmd` (not `.sh`) | 10 | people and programs on Windows | W | stays as stage-0, no longer needs Git Bash | WP-1, E1 |
| `tools/githooks/claude-pre-tool-use` and its source `tools/paddock-dev/src/hook/claude-pre-tool-use.sh` | 7 + 7 | Claude Code (`tools/githooks/claude-settings.json`); `hook/install.rs` embeds the source | M | stays | E3 |
| `tools/bootstrap.sh` | 112 | docs (Pitcrew, `tools/README.md`); `tools/test/bootstrap.test.mjs` | M L | `./dev app bootstrap <app>` | WP-7 |
| `scripts/setup-dev.sh` | 64 | `docs/dev-setup.md` | M | the rustup install stays (E2, about 15 lines); the checks become `./dev setup --check` | WP-7 |
| `ci-image/fetch-verified.sh` | 39 | `ci-image/Dockerfile` `RUN`; ci.yml:505 (draw.io in a job) | L | stays for the Dockerfile (E4); the ci.yml call becomes `./dev fetch` | WP-4 |

### 1.2 The CI kit (`conformance/ci`, `tools/`): Rust ports exist, the shell is the release-path fallback

The Rust ports are on `origin/main` (`tools/paddock-dev/src/ci_isolation`, `ci_build_jobs.rs`, `ci_host/*`), held equal by
`tests/it/host_steps.rs`, `build_jobs.rs`, `conformance/test/job-isolation.test.mjs` (`ISOLATION_CMD`) and `kept-clone.test.mjs`
(`KEPT_CLONE_CMD`). `kit.sh` picks the shell for a tag run, `PADDOCK_DEV_DOWNLOAD=0`, a checkout without the tool, no `rustc`, or
`PADDOCK_CI_KIT=shell` (`kit_choice` in `conformance/ci/kit.sh`). The copies are registered as `ci-job-isolation`, `ci-kit-shell`
and `ci-host-steps` in `docs/known-copies.toml`, each `delete_by = "v0.5.0"`.

| Script | Lines | Called by | OS | Rust home | WP |
|---|---:|---|---|---|---|
| `conformance/ci/kit.sh` | 238 | 12 actions and 15 workflows | M L W | deleted; callers run `./dev ci <step>` | WP-2 |
| `conformance/ci/job-isolation.sh` | 475 | `job-isolation` and `setup-rust` actions (kit); sourced by Debrief's `libduckdb-env.sh` and `windows-node-modules.sh` | M L W | `./dev ci isolation` (built) | WP-2 |
| `conformance/ci/job-tmpdir.sh` | 171 | `job-tmpdir` action; `job-isolation.sh` | M L W | `./dev ci job-tmpdir setup` and `remove` (new command over the rule `ci_isolation` already runs in process) | WP-2 |
| `conformance/ci/ci-home.sh` | 28 | sourced by `job-isolation.sh` | M L W | in `ci_isolation` (built) | WP-2 |
| `conformance/ci/build-jobs.sh` | 215 | `build-jobs` action (kit) | M L W | `./dev ci build-jobs` (built) | WP-2 |
| `conformance/ci/build-folder.sh` | 38 | `build-folder` action (kit) | M L W | `./dev ci build-folder` (built) | WP-2 |
| `conformance/ci/bounded.sh` | 44 | sourced by `kept-clone.sh` | M L W | `ci_host/phase.rs` (built) | WP-2 |
| `conformance/ci/boot-volume.sh` | 79 | `job-tmpdir.sh`; `crates/paddock-testkit/src/boot_volume.rs` runs it; Dyno's `check-bundle.sh` | M | one Rust rule in a shared crate, used by `./dev ci boot-volume` and `paddock-testkit` (ER-22) | WP-2 |
| `conformance/ci/guardian-as-user.sh` | 83 | ci.yml Linux crates job (kit) | L | `./dev ci guardian-as-user` (built) | WP-2 |
| `conformance/ci/kept-clone.sh` | 251 | `kept-clone` action (kit) | M L W | `./dev ci kept-clone` (built) | WP-2 |
| `conformance/ci/host/host-tools.sh` | 41 | `host-tools` action (kit) | M L W | `./dev ci host-tools` (built) | WP-2 |
| `conformance/ci/host/node-caches.sh` | 32 | `node-caches` action (kit) | M L W | `./dev ci node-caches` (built) | WP-2 |
| `conformance/ci/host/sccache-stats.sh` | 8 | `sccache-stats` action (kit) | M L W | `./dev ci sccache-stats` (built) | WP-2 |
| `conformance/ci/host/stable-workspace.sh` | 115 | `stable-workspace` action (kit) | M W | `./dev ci stable-workspace` (built) | WP-2 |
| `tools/prune-build-folders.sh` | 38 | `kit.sh host prune-folder` | M L W | `./dev ci prune-folder` (built) | WP-2 |
| `tools/prune-ci-build-folder.sh` | 22 | `kit.sh host prune-build-folder` | M L W | `./dev ci prune-build-folder` (built) | WP-2 |
| `tools/host-cleanup.sh` | 109 | `kit.sh host host-cleanup` | M L W | `./dev ci host-cleanup` (built) | WP-2 |

### 1.3 The release path

| Script | Lines | Called by | OS | Rust home | WP |
|---|---:|---|---|---|---|
| `conformance/ci/tauri-bundle.sh` | 138 | `tauri-bundle` action; app-release.yml | M L W | `./dev release bundle` (through `ci_tool::ToolCall`) | WP-3 |
| `conformance/ci/updater-key.sh` | 93 | `source`d in app-release.yml and debrief.yml:1078 | M L W | part of `./dev release bundle`: the key goes only to `tauri build` (ToolCall's pass list), so no shell has to strip it | WP-3 |
| `conformance/ci/first-release-notes.sh` | 84 | app-release.yml:291 and :467; `first-release-notes.test.mjs` | L | `./dev release notes --first` | WP-3 |
| `conformance/ci/tag-node-caches.sh` | 30 | app-release.yml:265, debrief.yml:1037 | M L W | `./dev ci tag-node-caches` | WP-3 |
| `conformance/ci/dev-tool-publish.sh` | 87 | dev-tool.yml:109 and :142; `tests/it/dev_tool_publish.rs` | L | `./dev ci dev-tool-publish` | WP-3 |
| `conformance/release/macos-verify-bundle.sh` | 156 | `macos-verify` action (Dyno, Pitwall releases, nightly); `macos-verify-bundle.test.mjs` | M | `./dev release macos-verify` | WP-3 |
| `apps/debrief/scripts/ci/macos-verify-bundle.sh` | 83 | debrief.yml:831 and :1184 | M | deleted; a second copy of the one above (rule 55), Debrief calls the shared command | WP-3 |
| `scripts/demo/bundle.sh` | 52 | shell-demo-release.yml:158, workbench-demo-release.yml:158, service-smoke.yml:92 | M L W | `./dev release bundle` | WP-3 |
| `scripts/demo/verify-macos.sh` | 83 | the two demo releases (:164) | M | `./dev release macos-verify` | WP-3 |
| `scripts/demo/check-agent-version.sh` | 23 | service-smoke.yml:96; `docs/gates.toml` | M | `./dev release check-version` | WP-3 |
| `apps/dyno/scripts/check-bundle.sh` | 161 | dyno-release.yml:126 and :178, nightly.yml:288 (`bundle-check:`); `docs/gates.toml` | M L W | `./dev release check-bundle dyno`, its app-specific rows as data in `apps/dyno/paddock.ci.json` | WP-3 |
| `scripts/notices.sh` | 22 | `app-gates` action:91, debrief.yml:498; `notices.test.mjs` | M L W | `./dev notices` (salvage `tools/dev-notices`) | WP-3 |

### 1.4 Other CI scripts

| Script | Lines | Called by | OS | Rust home | WP |
|---|---:|---|---|---|---|
| `apps/debrief/scripts/ci/libduckdb-env.sh` | 73 | ci.yml:1146, debrief.yml:204, :243, :365, :779, nightly; `libduckdb-env.test.mjs` | M L W | `cargo xtask libduckdb --github-env` (Debrief's xtask already has `libduckdb`) | WP-4 |
| `apps/debrief/crates/debrief-store-duck/scripts/fetch-libduckdb.sh` | 61 | debrief-bench.yml:132; `build-sidecars.mjs`, `libduckdb.mjs` | M L W | deleted; `cargo xtask libduckdb` is the one implementation | WP-4 |
| `apps/debrief/scripts/ci/windows-node-modules.sh` | 53 | debrief.yml:800, :924, :1048, :1053 | W | `cargo xtask node-modules link` and `unlink` | WP-4 |
| `apps/debrief/scripts/ci/macos-smoke.sh` | 101 | debrief.yml:832; `macos-smoke.test.mjs` | M | `cargo xtask macos-smoke` | WP-4 |
| `apps/pitwall/scripts/ci/privacy-check.sh` | 22 | `apps/pitwall/paddock.ci.json`, lefthook.yml:52 | M L W | a Rust check (`./dev check run`) or a Pitwall test | WP-4 |
| `ci-image/verify.sh` | 101 | ci-image.yml (inside the image, which has Rust) | L | `./dev ci image-verify` | WP-4 |
| `scripts/shell-demo/build-linux-arm64.sh` | 53 | service-smoke.yml:107; its `.test.mjs` | M (Docker) | `./dev demo build-linux` (uses `./dev linux-container`, WP-6a) | WP-4 |

### 1.5 Claude hooks

The tracked guard is already Rust: `tools/paddock-dev/src/hook` with the rules `no_pgrep_wait`, `no_force_push`,
`no_unbounded_wait`, `no_unscoped_cleanup` and `no_kill_by_name` (`rules/mod.rs`, `all()`). The older guards below are wired in
each checkout's untracked `.claude/settings.json` by the coordinator (their headers say so); each `.sh` only runs
`exec python3 -I -B <name>.py`.

| Script | Lines (sh + py) | Called by | OS | Rust home | WP |
|---|---:|---|---|---|---|
| `.claude/hooks/worktree-guard.sh` | 13 + 146 | local settings (PreToolUse) | M | rules `unguarded-cd`, `main-checkout-git-write` in `dev hook pre-tool-use` | WP-5 |
| `.claude/hooks/docker-guard.sh` | 19 + 211 | local settings | M | rule `docker-mass` | WP-5 |
| `.claude/hooks/kill-guard.sh` | 22 + 239 | local settings | M | extend `no_kill_by_name` with the pid proof (descendant of the session, cwd in the worktree) through `paddock_proc` | WP-5 |
| `.claude/hooks/scratch-root-guard.sh` | 8 + 130 | local settings | M | rule `scratch-root` | WP-5 |
| `.claude/hooks/no-force-push.sh` | 3 + 51 | local settings | M | deleted: `no_force_push` (built) | WP-5 |
| `.claude/hooks/no-pgrep-wait.sh` | 8 | local settings | M | deleted: `no_pgrep_wait` (built) | WP-5 |
| `.claude/hooks/git-output-filter-warn.sh` | 9 | local settings | M | a warning rule (the decision type gains "allow with context") | WP-5 |
| `.claude/hooks/recurring-guard.sh` | 6 + 143 | local settings (Stop) | M | `dev hook stop` | WP-5 |
| `.claude/hooks/lib/shellscan.py`, `lib/paddockpaths.py` | 702 + 79 | the Python guards | M | replaced by `hook/lex.rs` and `paddock_env` paths (built) | WP-5 |
| the eight `.claude/hooks/*.test.sh` | 685 | people; `c115-scoped-cleanup.test.mjs` names one | M | their cases move into corpus files like `hook/fixtures/corpus-no-force-push.tsv` | WP-5 |

### 1.6 VM, container and host harness (local only, except where named)

| Script | Lines | Called by | OS | Rust home | WP |
|---|---:|---|---|---|---|
| `scripts/vm-win.sh` | 424 | people; `vm-win.test.mjs` runs in ci.yml's VM tooling job; `docs/harnesses.toml` | M host | `./dev vm win` | WP-6a |
| `scripts/vm-win/prepare.sh` | 230 | `vm-win.sh`; tests | M host | `./dev vm win prepare` | WP-6a |
| `scripts/vm-win/target.sh` | 64 | `vm-win.sh`, `vm-smoke/win-vm-target.mjs`, `vm-win-path.mjs` | M host | `./dev vm win target` | WP-6a |
| `scripts/vm-lock.sh` | 180 | `vm-lock-run.sh`, `vm-checks/*`, `vm-lock-reexec.mjs`; `vm-lock.test.mjs` in ci.yml | M host | `./dev vm lock` on `paddock-slots` | WP-6a |
| `scripts/vm-lock-run.sh` | 14 | docs, `vm-lock-reexec.mjs` | M host | `./dev vm lock run` | WP-6a |
| `tools/macos-vm-smoke.sh` | 186 | service-smoke.yml:104 and :115; `macos-vm-smoke.test.mjs` | M host | `./dev vm tart-smoke` | WP-6a |
| `scripts/shell-demo/smoke-service-update.sh` | 90 | `macos-vm-smoke.sh` in the Tart guest; its `.test.mjs` | M guest | a Rust binary cross-built and copied into the guest, or an exception added to §2 (decided in WP-6a) | WP-6a |
| `scripts/shell-demo/smoke-service-update-linux.sh`, `smoke-desktop-linux.sh` | 67 + 85 | `macos-vm-smoke.sh` (Linux guest) | L guest | as above | WP-6a |
| `scripts/vm-smoke/vm-smoke.sh` | 7 | docs, releasing guides | M host | deleted: it only runs `node run.mjs` (the JS harness is ci-in-rust WP-14) | WP-6a |
| `scripts/vm-smoke/chain-release.sh`, `login-probe-linux.sh`, `login-probe-mac.sh`, `make-no-dyno-snapshot.sh` | 48 + 56 + 76 + 24 | docs, `win-vm-target.test.mjs`, `vm-win.test.mjs` | M host | `./dev vm` subcommands | WP-6a |
| `scripts/vm-checks/dyno-wp6-cross-account-pipe.sh` | 60 | named by `crates/paddock-ipc/tests/it/system.rs` | M host | `./dev vm check cross-account-pipe` | WP-6a |
| `scripts/disk-record.sh` | 10 | `vm-smoke/disk-record.mjs`, `disk-record.test.mjs` | M | deleted: callers run `./dev disk record` | WP-6a |
| `tools/linux-container.sh` | 83 | the app wrappers below; `linux-container.test.mjs` | M (Docker) | `./dev linux-container` | WP-6a |
| `apps/dyno/scripts/linux-container.sh`, `linux-verify.sh` | 51 + 16 | docs | M (Docker) | `./dev linux-container --app dyno` | WP-6a |
| `apps/pitwall/scripts/linux-container-verify.sh`, `linux-container-inner.sh` | 28 + 68 | docs (the inner one runs in the container) | M, L container | `./dev linux-container --app pitwall` | WP-6a |
| `apps/debrief/scripts/ci/e2e-linux-local.sh`, `smoke-linux-docker.sh` | 45 + 51 | docs | M (Docker) | `./dev linux-container --app debrief` profiles | WP-6a |
| `crates/paddock-setup/nsis/test/run-harness.ps1`, `run-dyno-e2e.ps1` | 313 + 88 | `docs/harnesses.toml` | W guest | `paddock-vm-guest nsis-harness`, `dyno-e2e` (xwin-built, run in the VM) | WP-6b |
| `scripts/vm-smoke/win-suite.ps1` | 227 | `run-win.mjs`, `win-cdp.mjs`, `win-rows.mjs`, `win-tray.mjs`, `win-ui.mjs` | W guest | `paddock-vm-guest suite` | WP-6b |
| `scripts/vm-smoke/win-act.ps1` | 128 | `run-win.mjs`, `win-act.mjs`, `win-snap.mjs`, `win-tray.mjs`, `win-tray-icons.mjs` | W guest | `paddock-vm-guest act` (SendInput, foreground window, screenshots through the `windows` crate) | WP-6b |
| `scripts/vm-smoke/win-gui-probe.ps1`, `win-menu-control.ps1` | 72 + 20 | docs; `run-win.mjs`, `win-tray.mjs` | W guest | `paddock-vm-guest gui-probe`, `menu-control` (UI Automation, `Shell_NotifyIcon` and `TrackPopupMenu`) | WP-6b |
| `scripts/vm-win/ci-like.ps1` | 109 | `vm-win.sh run --ci-like` | W guest | `paddock-vm-guest ci-like` | WP-6b |

### 1.7 Developer and app scripts

| Script | Lines | Called by | OS | Rust home | WP |
|---|---:|---|---|---|---|
| `diagrams/scripts/export-diagrams.sh` and four identical copies (Debrief, Pacenotes, Sighter, `templates/repo`; same MD5) | 5 x 41 | docs, `.gitea/actions/README.md` | M | `./dev diagrams export`; four copies deleted (rule 55) | WP-7 |
| `apps/debrief/docs/diagrams/src/generate.sh`, `apps/dyno/docs/diagrams/src/build.sh`, `export.sh` | 6 + 7 + 34 | docs | M | `./dev diagrams generate --app` and `export --app` | WP-7 |
| `tools/verify-installed.sh` | 117 | docs; `verify-installed.test.mjs` | M L | `./dev app verify-installed <app>` | WP-7 |
| `apps/pitcrew/scripts/{linux,macos}-{bootstrap,verify}.sh` | 63 + 62 + 56 + 56 | docs | M L | deleted after parity with `./dev app bootstrap pitcrew` and `verify-installed pitcrew` | WP-7 |
| `apps/pitwall/scripts/linux-bootstrap.sh`, `linux-verify.sh`, `macos-verify.sh` | 61 + 57 + 106 | docs | M L | deleted after parity, as above (Pitwall's macOS check is longer: its extra rows become app data) | WP-7 |
| `apps/dyno/scripts/regen-wire-fixtures.sh` | 13 | `pre_push/gates.rs`, `dyno-core/tests/wire` | M L W | a `./dev gen` generator entry | WP-7 |
| `apps/dyno/scripts/linux-units-check.sh` | 24 | `docs/gates.toml` (tool gate) | L host | Dyno xtask `linux-units` (new `apps/dyno/xtask`, the Debrief pattern) | WP-7 |
| `apps/dyno/scripts/bench-smoke.sh`, `verify-tool-pin.sh` | 35 + 33 | docs | M | Dyno xtask `bench-smoke`, `verify-tool-pin` (gpg through ToolCall) | WP-7 |
| `apps/debrief/scripts/build-plugins.sh` | 43 | docs (`build-sidecars.mjs` does it for releases) | M L W | deleted, or a Debrief xtask line if a person still needs it | WP-7 |
| `apps/debrief/scripts/macos/build-app.sh`, `dev-signing.sh` | 17 + 58 | docs | M | Debrief xtask `dev-sign` | WP-7 |
| `apps/debrief/scripts/perf/perf-import.sh`, `perf-post.sh` | 28 + 30 | `docs/performance.md`, `paddock.conformance.json` | M | Debrief xtask `perf` | WP-7 |
| `apps/debrief/crates/debrief-pdf/fonts/build-fonts.sh` | 49 | docs, `fonts.rs` comment | M | Debrief xtask `fonts` | WP-7 |
| `crates/paddock-setup/nsis/test/build.sh` | 14 | docs | M L | `./dev setup-harness build` | WP-7 |
| `scripts/perf/link-time/cc-timed.sh`, `disfn.sh` | 26 + 9 | research note, `performance.md` | M | deleted; the research note links the commit (D8) | WP-7 |
| `apps/debrief/docs/research/2026-10-28-duckdb-server-mode/harness/bench.sh` | 123 | research notes | M | deleted, the note links the commit (D8) | WP-7 |
| `apps/dyno/scripts/wp0/wp0-run.sh` and three `.ps1` | 91 + 4 + 4 + 156 | `wp0/PLAN.md`, Dyno ADR 0033, `performance.md` | M host, W guest | deleted (D8) | WP-7 |
| `apps/pitcrew/scripts/windows-retire-legacy-runner.ps1` | 142 | docs | W | deleted (D8) | WP-7 |
| `apps/debrief/scripts/clean-build-dirs.ps1` | 56 | docs | W | deleted: `./dev ci prune-agent-builds` (built) | WP-7 |

### 1.8 What stays as data or guest code

| Script | Lines | Called by | Why it stays | Exception |
|---|---:|---|---|---|
| `tools/heavy/tests/fixtures/plock.sh` | 281 | `tools/heavy/tests/it/mixed.rs` | the reference shell lock for mixed shell and Rust runs on one lock folder; deleted when no machine runs the shell plock | E6 |
| `tools/paddock-dev/src/hook/fixtures/no-force-push.sh`, `no-pgrep-wait.sh` | 8 + 8 | `hook/oracle_tests.rs` | oracle copies of the old hooks; deleted in WP-5 when the corpus files hold all their cases | E6 |
| `tools/paddock-dev/tests/golden/**/build-folder.sh`, `paths.sh` (3) | 4 + 3 + 3 | the planner's golden test | planner input fixtures, not tools | E6 |
| `scripts/vm-win/firstlogon.ps1`, `admin-setup.ps1`, `gui-setup.ps1`, `logon-runner.ps1` | 106 + 21 + 20 + 23 | `vm-win.sh`, `prepare.sh`, `autounattend.xml.in` | provisioning at first logon, before our binaries exist in the guest | E5 |
| `apps/dyno/app/src-tauri/linux/postinst`, `postrm`, `prerm` | small | dpkg | Debian maintainer scripts | E7 |

## 2. What stays shell, and why

- **E1. Stage-0 launchers `dev` and `dev.cmd`.** Something must start before any Rust binary exists. Each is about 20 lines and
  uses only builtins plus `cargo` and `rustup`: find the root, run `$PADDOCK_DEV` when it is set, else run the launcher crate with
  `cargo run -q --release --manifest-path tools/dev-launch/Cargo.toml -- "$@"`. When `cargo` cannot run and `rustup` exists, run
  the launcher once with an installed toolchain (`RUSTUP_TOOLCHAIN` set to the first line of `rustup toolchain list`, read with
  `read`) so the launcher can install the pin. No `grep`, `tr`, `sed`, `awk` or `ps`. `dev.cmd` no longer needs Git Bash.
- **E2. Installing Rust.** `scripts/setup-dev.sh` keeps the rustup install (about 15 lines); nothing in Rust can install Rust.
  Everything else in it becomes `./dev setup --check`.
- **E3. The Claude hook shim** (`tools/githooks/claude-pre-tool-use` and its source). Claude Code runs a command line per tool
  call. The shim finds the installed binary and `exec`s it, with builtins only (about 10 ms; the `./dev` wrapper costs 150 ms or
  more per call, `hook/mod.rs`). It already exists and stays as it is.
- **E4. `ci-image/fetch-verified.sh` in the Dockerfile.** It runs in `RUN` lines while the image is built, before Rust is in it.
- **E5. Four provisioning scripts in a fresh Windows guest** (`scripts/vm-win/firstlogon.ps1`, `admin-setup.ps1`, `gui-setup.ps1`,
  `logon-runner.ps1`, 170 lines). They run at first logon, from the unattended setup, before any of our binaries is in the guest.
  Every other PowerShell file is ported (D5, WP-6b).
- **E6. Test fixtures.** Files that are the input or the oracle of a test, not tools. They live under `tests/` or `fixtures/`.
- **E7. Debian maintainer scripts.** dpkg runs them; Debian Policy §6 asks for "proper executable files" starting with `#!` and
  `set -e` for shell. They are a few lines each and call nothing of ours.

Lefthook entries stay one-line `./dev ...` calls (lefthook is configuration, not a script). Inline workflow shell is D7.

## 3. Common practice (ER-37)

Read on 2026-11-15.

- **cargo-xtask** (<https://github.com/matklad/cargo-xtask>): a `.cargo/config.toml` alias `xtask = "run --package xtask --"`.
  It "fully bootstraps" from cargo and rustc and "doesn't use the shell", which is what makes it portable. Debrief already uses it
  (`apps/debrief/.cargo/config.toml:8`).
- **rust-lang/rust** (<https://raw.githubusercontent.com/rust-lang/rust/master/x>,
  <https://rustc-dev-guide.rust-lang.org/building/bootstrapping/how-bootstrap-does-it.html>): `./x` is a POSIX `sh` launcher of
  about 60 lines that finds Python and runs `x.py`; `x.ps1` is the Windows twin. `x.py` and `bootstrap.py` then build and run the
  Rust bootstrap (`src/bootstrap/src/bin/main.rs`), where the build logic lives. So: a small shell stage 0 per OS, logic in Rust.
  WP-1 is the same shape, without Python.
- **Deno** (<https://docs.deno.com/runtime/reference/cli/task/>): `deno task` runs its own "cross-platform shell that's a subset
  of sh/bash", with built-in `cp`, `mv`, `rm`, `mkdir`, `cat`, `head`, `xargs` and others that "work the same out of the box on
  Windows, Mac, and Linux". The tool removes the dependence on the system's shell tools, which is the class of run 8138.
- **Bun** (<https://bun.com/docs/runtime/shell>): Bun Shell runs inside the Bun process with native `cd`, `ls`, `rm`, `cat`,
  `mkdir`, `mv`, `which` and others, for the same reason (it lets projects drop `rimraf` and `cross-env`).
- **uv** (<https://github.com/astral-sh/uv/tree/main/scripts>): about 15 `.sh`, about 28 `.py` (run through uv with lock files),
  one `cargo.cmd` and one `.ps1` in `scripts/`, mostly release and check chores. A large Rust project keeps scripts beside the code
  and does not port them to Rust.
- **The platform vendor** (<https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax>, `defaults.run.shell`):
  `pwsh` "is the default shell used on Windows"; `shell: bash` on Windows uses "the bash shell included with Git for Windows".
  Gitea Actions follows that syntax. Neither asks for logic to leave shell. Microsoft ships `curl.exe` and `tar.exe` in Windows 10
  from build 17063 (<https://devblogs.microsoft.com/commandline/tar-and-curl-come-to-windows/>), which the launcher's download
  uses.

**Where this plan goes beyond common practice.** Porting every script is more than uv or rust-lang do; a stage-0 shell plus Rust
logic is what rust-lang and xtask do. The content-addressed binary (ADR 0022 §3.3) is ours alone; WP-1 keeps it, which costs about
1.5 days more than a plain xtask alias (D3).

**The cheaper common-practice option** (D1): WP-0, WP-1 and WP-2 only (8.5 days). That removes both run 8138 shell causes, the
duplicate kit and new shell, and leaves the other scripts in shell until each is next changed ("port on touch", as uv keeps its
scripts). The full plan is 39 days.

## 4. Work packages, by risk

Every WP: the script's callers switch in the same change that deletes the script (ADR 0022 §3.7, "one command at a time"); its
`docs/known-copies.toml` entry goes with it; the new check's baseline (WP-0) shrinks by the deleted files. Tests run on macOS, in
the Linux CI image (`docker run --rm --cpus 4 --memory 12g` with the pinned `paddock-ci` image, ER-14) and on Windows through the
test VM under `scripts/vm-lock.sh` (later `./dev vm lock`) or the Windows CI leg of the `paddock-dev` job.

| WP | Content | Files | Tests | Build | Review | Total (range) | Depends on |
|---|---|---|---|---:|---:|---:|---|
| **WP-0** | **The check that keeps new shell out** (section 6), report-only, with today's files as a shrink-only baseline | new check in `tools/paddock-dev/src/check/`, its baseline and exception list, `conformance/paddock.ci.json` line | a good and a bad fixture per file kind; the check shown red on a planted `.sh` (ER-33) | 0.75 | 0.25 | **1** (0.75-1.5) | – |
| **WP-1** | **The launcher.** `tools/dev-launch` (std only): input hash (the code of `tools/paddock-dev/src/input_hash.rs` moves here and `paddock-dev` depends on it: one copy, rule 55), compiler and config discovery, the toolchain install under `File::lock` (std 1.89, `flock` / `LockFileEx`, released with the handle: no pid to verify, rule 53) with the `toolchain_broken` checks, download through `curl` with the token on stdin, build, the one restart when the compiler changes, `run_bounded`. Stage-0 `dev` and `dev.cmd` (E1). An ADR 0022 amendment for §3.2 and §3.3 | `dev`, `dev.cmd`, `tools/dev-launch/**`, `tools/paddock-dev/src/input_hash.rs`, `tools/paddock-dev/Cargo.toml` and lock, `.gitea/actions/setup-rust` (`--install-toolchain` call) | golden of the hash against today's wrapper on the same tree, env and configs (both run, same hash); a run with PATH holding only `cargo` (the run 8138 shape) on all three OS; the lock with two concurrent installs; a toolchain that changes mid-build; measured per-call time against docs/benchmarks.md's 0.16 s | 3.5 | 0.5 | **4** (3-6) | – |
| **WP-2** | **Retire the kit's shell** (the kit.sh and prune duplicate). Every action calls `./dev ci <step>`; release, tag and rule 54 runs build `./dev` from source (`PADDOCK_DEV_DOWNLOAD=0`, already supported) instead of taking the shell; `./dev ci job-tmpdir`; boot-volume's rule in one shared crate for `./dev` and `paddock-testkit`; delete `kit.sh` and the 17 scripts of §1.2; the parity tests become goldens of the Rust output; known-copies `ci-job-isolation`, `ci-kit-shell`, `ci-host-steps` removed. A rule 54 dispatch of each app's release and both demos with gates on and bundles built | `conformance/ci/**`, `tools/*.sh`, `.gitea/actions/*` (12), workflows that call `kit.sh` (15), `crates/paddock-testkit/src/boot_volume.rs`, Dyno `check-bundle.sh` (one line) | the existing `host_steps.rs`, `build_jobs.rs`, `job-isolation.test.mjs`, `kept-clone.test.mjs` cases kept as goldens; the prune case on a loop-mounted ext4 in the CI image (`ffe91b1ac`'s reproduction); rule 54 dry runs green | 2.5 | 0.5 | **3.5** (2.5-5) | WP-1 for Windows callers without Git tools; `fix/c125-and-arm64-tests` and `fix/stale-disk-paths` landed |
| **WP-3** | **Release-path scripts** of §1.3: `./dev release bundle` (tauri-bundle, demo bundle, updater key passed only to the bundler), `release macos-verify` (Debrief's copy deleted), `release notes --first`, `release check-version`, `release check-bundle dyno`, `ci tag-node-caches`, `ci dev-tool-publish`, `./dev notices` (salvage `tools/dev-notices`). A rule 54 dry run per app and demo | §1.3 files, `app-release.yml`, `debrief.yml` release jobs, `dyno-release.yml`, `pitwall-release.yml`, demo releases, `dev-tool.yml`, `macos-verify`, `tauri-bundle`, `app-gates` actions | the `.test.mjs` suites pointed at the command (`*_CMD`, as `ISOLATION_CMD`) before their cases move to Rust; a test that the updater key reaches no child but the bundler; the bundles of the dry run inspected (rule 54) | 5 | 1 | **6** (4.5-8) | WP-2 |
| **WP-4** | **Other CI scripts** of §1.4 and the ci.yml draw.io call: Debrief's four through its xtask, Pitwall's privacy check, `ci image-verify`, `demo build-linux`; `./dev fetch` (salvage `tools/dev-fetch-r3`) for draw.io | §1.4 files, `ci.yml`, `debrief.yml` (non-release jobs), `debrief-bench.yml`, `ci-image.yml`, `service-smoke.yml`, `lefthook.yml` (one line), `apps/pitwall/paddock.ci.json` | `libduckdb-env.test.mjs` and `macos-smoke.test.mjs` against the command; one CI run of each job that used a script | 2.5 | 0.5 | **3** (2.25-4.5) | WP-2 (same actions and workflows) |
| **WP-5** | **Claude hooks**: the six guards of §1.5 as rules of `dev hook pre-tool-use` and a `dev hook stop`; the Python and `.test.sh` files deleted; the coordinator rewires local settings to the one shim | `tools/paddock-dev/src/hook/**`, `.claude/hooks/**`, `tools/githooks/claude-settings.json` | each `.test.sh` case as a corpus row, run against the old hook first (oracle) and then the rule; the kill proof on a real child tree on macOS and Linux | 4 | 0.5 | **4.5** (3.5-6) | – |
| **WP-6a** | **VM, container and host harness shell** of §1.6: `./dev vm win`, `vm lock`, `vm tart-smoke`, `vm check`, `./dev linux-container --app`; the Tart guest scripts decided per file (a cross-built binary copied in, or an exception added to §2) | §1.6 files, `docs/harnesses.toml`, `service-smoke.yml` (two lines) | `vm-win.test.mjs`, `vm-lock.test.mjs`, `macos-vm-smoke.test.mjs`, `linux-container.test.mjs` against the command; one locked VM run and one Tart smoke run | 4 | 0.5 | **4.5** (3.5-6.5) | WP-1 (target.sh's env handling moves into the launcher's contract); train 12 |
| **WP-6b** | **The Windows guest PowerShell** (D5): one program `paddock-vm-guest`, built with `cargo xwin` for `x86_64-pc-windows-msvc`, copied into the VM by `./dev vm win`, with the subcommands `nsis-harness`, `dyno-e2e`, `suite`, `ci-like`, `act`, `gui-probe` and `menu-control`. The GUI parts use the `windows` crate (0.62, already in the family): UI Automation (`IUIAutomation`), `SendInput`, window enumeration, GDI screenshots, `Shell_NotifyIcon` and `TrackPopupMenu`; OS state through WMI or `powercfg` by `ToolCall`. The JS callers call the program instead of `powershell -File` | the 7 `.ps1` files of §1.6, a new crate under `scripts/vm-smoke/guest/` (or `tools/`), the JS callers named in §1.6, `docs/harnesses.toml` | the old script and the program side by side in one locked VM session on the same build, comparing the step JSON and logs after normalisation; the UI probe shown red on a hidden window and a missing menu (ER-33); host unit tests of the pure parts; `cargo xwin clippy` | 5 | 0.5 | **5.5** (4.5-7) | WP-6a; train 12 |
| **WP-7** | **Developer and app scripts** of §1.7, in two lanes: (a) shared: `./dev diagrams`, `app bootstrap`, `app verify-installed`, `setup --check`, `gen` entry for Dyno's wire fixtures, deletions of the app copies and of D8's one-offs; (b) app xtasks: Debrief's chores, a new `apps/dyno/xtask` | §1.7 files, `apps/dyno/.cargo/config.toml`, `apps/*/docs` that name the scripts | parity with the old script on a fixture app (stdout and exit code; for verify-installed a fake install tree); `bootstrap.test.mjs` and `verify-installed.test.mjs` against the command | 6 | 1 | **7** (5-10) | WP-1 |
| | **Total** | about 9,700 shell and 1,690 Python lines retired; about 550 stay (E1 to E7) | | **33.25** | **5.25** | **39** (29.5-54.5) | |

The ranges are summed end to end, so the high end is pessimistic.

**WP-6b's estimate, by part.** The NSIS harness and its Dyno end-to-end run (401 lines: process runs, exit codes, files left
behind) 1 day; the suite and `ci-like` (336 lines: WMI process checks, `powercfg`, the runner-like environment) 1.25; `act`,
`gui-probe` and `menu-control` (220 lines, but new Win32 code: `origin/main` has no UI Automation in Rust, so this part carries the
most risk) 1.75; the build, the copy into the VM and switching the JS callers 0.5; the side-by-side runs in a locked VM 0.5; review
0.5. The rough guess was 3 days; the firm figure is 5.5, which is more than 20% over it, so it goes back to the user (ER-37). Common
practice for automation inside a Windows guest is PowerShell, Microsoft's own scripting language; porting it costs these 5.5 days
beyond keeping it.

**Lanes at about 5 agents.** The shared files are `.gitea/actions/*` and the workflows (WP-2, WP-3, WP-4), `tools/paddock-dev/src/cli.rs`
(one declared line per new subcommand, merged by the integrator), `tools/paddock-dev/Cargo.toml` and its lock (WP-1 only), and the
WP-0 baseline (each WP removes its own lines). So:

| Lane | WPs | Days |
|---|---|---:|
| A, CI path | WP-2, then WP-3, then WP-4 (serial: the same actions and workflows) | 12.5 |
| B, launcher | WP-0 first, then WP-1, then WP-6a | 9.5 |
| C, hooks and guest | WP-5, then WP-6b (both after train 12) | 10 |
| D, shared dev scripts | WP-7a (after WP-1) | 3.5 |
| E, app xtasks | WP-7b | 3.5 |

Lane A is the critical path: about 12.5 working days plus about 15% for reviews and CI between WPs, so about 3 weeks. WP-2 can start
before WP-1 lands if its Windows callers keep running under Git Bash until then. Under ER-1 a lane that changes the release path
(A) waits whenever a release is in flight.

**How this changes ci-in-rust.md.** This plan takes the shell of its WP-4 (host actions, built), WP-6 (libduckdb fetch), WP-8
(Debrief's and Pitwall's shell), WP-9 (the isolation family), WP-11 (the release shell scripts; its JS and inline YAML stay there),
WP-12 (`ci-image/verify.sh`, link-time scripts) and the shell files of WP-14. Its JS, conformance engine and inline-YAML work stay
in that plan. The integrator updates its table as each WP here lands.

## 5. Parity and the switch-over

- **Two runs, one fixture.** For each script, a test runs the old script and the new command on the same input folder, the same
  environment and the same shim programs (`PATH` with recorded fakes for `docker`, `diskutil`, `tauri`, `gpg` and so on), and
  compares what reaches a caller: exit code, stdout and stderr after a declared normalisation, `$GITHUB_ENV`, `$GITHUB_PATH`,
  `$GITHUB_OUTPUT`, files written, and each shim call with its arguments and the variables that steer it. This is the method of
  ci-in-rust.md §7 and `tests/it/host_steps.rs`, which already holds it for the kit.
- **The probe must see a difference** (ER-33). Each parity test is shown red on a planted difference before it counts.
- **Parity is not correctness.** Run 8138's prune case had parity and was wrong on ext4. So each parity test also runs on the
  file systems and tools of all three OS (APFS on the Mac, ext4 in the CI image on a loop mount, NTFS on Windows), not only on the
  Mac.
- **When the script is deleted**, its side of the comparison becomes a golden file of the Rust output, and the test stays. A
  script with its own `node --test` suite is pointed at the command through one variable (`*_CMD`) first, then its cases move into
  Rust.
- **Switch in one change, no long fallback.** The kit kept a shell fallback "until the release-path lane" and the copy was where
  the ext4 bug lived twice. So a mutating step switches its callers and deletes the script in one PR, after its parity tests are
  green on all three OS and one CI run of the jobs that use it is green. The fallback is `git revert` of that PR. A read-only step
  (a check, a verify, a report) may run beside the old script for one train: a difference prints `::warning::` with the diff and
  fails only the shadow step; the script goes when the train lands. Release-path WPs (WP-2, WP-3) add the rule 54 dry run of every
  app and demo before landing.
- **No flag day.** Each WP lands on its own through a train. `main` never has a caller without its script or command, because the
  same commit switches the callers and deletes the file, and the check of WP-0 fails when a workflow, action, `paddock.ci.json`,
  `lefthook.yml` or script names a script path that is not tracked (section 6).

## 6. The check that keeps new shell out

A Rust check in `./dev check run` (built on `tools/paddock-dev/src/check`, on `origin/main`; its id from
`./dev check new-id --reserve <branch>` when WP-0 starts). It fails on any tracked file named `*.sh`, `*.bash`, `*.zsh`, `*.ps1`,
`*.psm1`, `*.py`, `*.cmd` or `*.bat`, or starting with a `#!` line that names `sh`, `bash`, `zsh`, `python` or `pwsh`, unless:

- it is in the declared exception list (a TOML file beside the check: path or glob, exception id E1 to E7, reason, owner; read
  through the check's own accessor, ER-42), or
- it is in the shrink-only baseline of today's files (`check/baseline.rs`: entries only ever leave; a file that grew is refused by
  `ensure_shrink_only`).

It also reports, without failing, the count of inline `run:` lines in `.gitea` (today 947 in workflows and 361 in actions), so D7
can be decided on a number. A second part fails when `.gitea`, a `paddock.ci.json`, `lefthook.yml` or a tracked script names a `.sh` or `.ps1` path that is not
tracked, so a deleted script cannot keep a caller. It runs on every change in the conformance job (ER-41; `ci/conformance-every-change`,
merged into train 12 at `cfdce4693`, not yet on `origin/main`), and
it starts report-only: it blocks only after its first green run on the real tree, recorded by run id next to its definition. This
builds ADR 0022 §3.7 (a) and (b), which are planned but have no enforcing code on `origin/main` (ci-in-rust.md §5 says so; no
check under `tools/paddock-dev/src/check` or `conformance/src/checks` counts tooling files).

## 7. Risks

- **The release path changes before the tags** (WP-2, WP-3). ADR 0022 G.7 kept it unchanged until after the tags; the user's
  2026-11-15 decision moves it earlier. Mitigation: rule 54 dry runs of every app and demo per WP, and ER-1 (no release-path landing
  while a release is in flight). D2 asks the user to confirm.
- **Release jobs build `./dev` from source** once the shell is gone. ADR 0022 §3.5 estimates 40 to 60 s cold; dev-tool.yml's header
  says about 100 s; neither is measured. WP-2 measures it on the dry runs and records it in docs/benchmarks.md (rule 48).
- **The launcher is new code on every call path.** WP-1 lands alone, with the hash golden against today's wrapper, so a wrong hash
  shows as a test failure and not as CI downloading the wrong binary.
- **Windows without Git Bash.** After WP-1 the launcher needs no Git tools, but scripts not yet ported still do. Until a script is
  ported, its Windows callers keep Git Bash on PATH, as today.
- **Windows guest programs** (WP-6b): UI Automation from Rust is new in the family. A probe that sees less than the PowerShell one
  would turn GUI rows into "not seen"; the side-by-side run in the VM and the planted red cases catch that before a script goes.
- **Tart guest scripts** (WP-6a) may need a cross-built binary per guest OS; WP-6a decides per file and may keep
  them as exceptions.

## 8. Decisions taken (user, 2026-11-15)

1. **D1, scope:** the full plan, WP-0 to WP-7, before the releases.
2. **D2:** the release path changes before the tags, with rule 54 release dry runs (WP-2, WP-3).
3. **D3:** the std-only Rust launcher behind a short stage 0 (WP-1).
4. **D4:** each script switches in one change; `git revert` is the fallback. A one-train shadow only for read-only steps.
5. **D5, changed from the recommendation:** the PowerShell logic is ported too. Only the four provisioning scripts stay (E5). The
   NSIS harness, the VM suite, the GUI probes and `ci-like.ps1` become `paddock-vm-guest`, built with xwin and run in the VM
   (WP-6b, 5.5 days; the rough guess was 3, so the firm figure goes back to the user under ER-37).
6. **D6:** WP-5 ports the Python hooks.
7. **D7:** inline workflow shell is counted by WP-0's check, not ported here (ci-in-rust.md WP-7 and WP-11).
8. **D8:** all listed one-offs are deleted without a port: Dyno's `wp0/*`, Pitcrew's `windows-retire-legacy-runner.ps1`, the
   DuckDB research `bench.sh`, the link-time scripts and Debrief's `clean-build-dirs.ps1` (WP-7).
9. **D9:** the ADR 0022 amendment for §3.2 and §3.3 is written in WP-1.
