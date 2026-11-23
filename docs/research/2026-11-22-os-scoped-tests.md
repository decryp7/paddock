# OS-scoped tests: a test runs on an OS only where its code does (2026-11-22)

- Status: research note for the user's decision. Nothing is built. Read-only: no builds, no CI dispatch.
- Requirement (user, 2026-11-22, repeated): test OS-specific code only on its own OS runner and run everything else on the Linux
  runners. ADR 0080 set "Linux first" on 2026-11-09; it was never carried through to the Rust legs.
- Heads read (ADR 0091), re-read before the commit: `origin/main` 67ac6d6ece36, `origin/ci/staged-fast-then-long` 13d02a1ca0e0
  (not landed), `origin/docs/os-only-failures-research` c09bbaf854e7 (ADR 0109, proposed), `origin/feat/schedule-reuse`
  9974dbd02734 (ADR 0113, accepted 2026-11-21, not on main), `origin/fix/windows-golden-planner-spawns` 58ae90f1127d (the precedent, its commit 86a0cacfb).
- Related: ER-7, ER-9, ER-33, ER-37, ER-41, ER-46, ER-48; ADR 0080, 0099, 0100, 0109, 0113; rule 48 (numbers go to benchmarks.md
  when this is built).

## 0. The rule, made precise

Taken literally, "the code runs on that OS in production" keeps almost every shared crate on Windows, because Dyno ships there.
What the user means (and what this note proposes) is:

> A test runs on Windows or macOS only when the code it tests **runs on that OS in production and behaves differently there**:
> it has `cfg(windows|unix|target_os)` code, calls an OS API crate (`windows-sys`, `libc`, `objc2`, `core-foundation`,
> `security-framework`, `zbus`), or goes through the portable layer of ADR 0109 (processes, paths, files, locks, environment).
> Everything else is portable logic and runs on Linux only.

"Production" for paddock-dev is the CI runner (all three OS) for the `./dev` wrapper and the `./dev ci <host step>` commands, and
the dev Mac for the agent tools. Linux already runs every test today, so nothing new runs on Linux: the change only removes
duplicates from the Windows and macOS legs (no extra Linux minutes, no extra load on d-gtr9).

## 1. Inventory (origin/main 67ac6d6ec)

Counts are `git grep` of `cfg(windows|unix|target_os|target_family)`/`cfg!(...)` and files using an OS-API crate (upper bounds).

| Workspace | Production OS | OS-specific code | Proposed host scope |
|---|---|---|---|
| `crates/` (66 crates) | inside the apps: Win, macOS, Linux | heavy: paddock-proc 204/17, testkit 98/11, service 88/60, ipc 86/15, env 59/13, secrets 43, mcp 42; some: window, dialogs, storage, startup, setup, archive, chromium, testkit-start, files, diag, panels, download, shell, metrics, menu, power, plugin-package, updater, plugin-host, slots, jobs, settings; process-spawning without cfg: paddock-git | Win and macOS: those ~31 crates, minus pure-model modules (paddock-service `model`, `model_linux`, `scenarios`, `resume_tests`; paddock-diag `perf`). Linux only: ~35 crates with no OS code (names, log, limits, guard, net, cli, build, bench, runtime, widgets, siblings, health, live, channels, commands, assistant, view, testplan, ...) |
| `apps/dyno` | Win, macOS, Linux | platform 189/40, agent 138/24, app 106/16, helper 59/7; core 30/3, bench 26/3, cli 9/2 (small cfg) | host: dyno-platform, -agent, -app, -helper, -helper-service (its `hostRustChecks` already lists four of them); Linux only: core, bench, bench-gpu, cli, plugin-api, plugin-host, example |
| `apps/pitcrew` | Win, macOS (service) | pitcrew-service 112/14, app 35/5 | host: service, app; Linux only: core, notify-channels |
| `apps/pitwall` | Win, macOS, Linux | pitwall-server 43/8, app 43/7 | host: server (app is excluded on hosts today); Linux only: core, cli, loopback, service (1/0) |
| `apps/spotter` | Win, macOS, Linux | render 28/2, sources 14/2, app 27/2 | host: render, sources, app; Linux only: core, engine (1/0) |
| `apps/debrief` (debrief.yml) | Win, macOS, Linux | ipc 117/12, app 99/17, chromium-driver 40, plugin-native 24, core 20, plugin-dotnet 20, agent, ai, plugin-tools, plugin-windows; 18 crates with none | macOS `rust tests` job: the OS crates only; Windows already runs only smoke and bundle (product-level, unchanged) |
| shell-demo, workbench-demo | Linux leg only | — | unchanged |
| `tools/paddock-dev` (one crate) | see below | 422/59 | by module, below |
| npm: `ui`, `conformance`, `eslint`, `icons`, tools scripts, app UIs (vitest) | Linux CI, dev Mac | none | Linux only (already) |
| Playwright: reference apps and app e2e (Chromium) | Linux | — | Linux (already) |
| Playwright WebKit (`e2e-webkit`, app `e2eWebkit`) | macOS (Tauri's WebView there) | engine | macOS (already; correct) |
| `tools-macos` "VM tooling tests" (`node --test scripts/vm-lock*.mjs vm-win* member-gate*`) | the local test VMs, retired 2026-11-20 | — | decision D3: delete, or move to Linux |

**paddock-dev by module.** Host-runner production (all three OS): the `./dev` wrapper and tool cache (`tool_cache`,
`input_hash`, `wrapper` tests), `ci_setup_rust`, `ci_s3_guard`, `ci_build_jobs`, `ci_build_env`, `ci_rust_pin`, `ci_run`
(the gate executor), `ci_host/*` (stable workspace, kept clone, build folder, prune, sccache stats, host cleanup, boot volume on
macOS), `ci_isolation/*`, `slots`, `release_isolation`, `safe_remove`, `wait`/`wait_lock`, `child_env`, `process_env`,
`git_env`, `places`, `pathfind`, `load`, `cli`, and every module these import (today `ci_setup_rust` imports
`local_sccache::server_env`, and `ci_run` imports `pre_push::exec::heavy_prefix`, so parts of `local_sccache` and
`pre_push::exec` are host production too). Dev Mac only: `pre_push`, `worktree`, `hook`, `disk`, `local_sccache` (the rest),
`merge_check`, `queue`. Linux CI or dev Mac, portable logic: planner (`plan*`, goldens), `check/*` (conformance), `gates/*`,
`generators`, `ci_runs/*` (API client, verdict, triage, trial), `audit`, `check_ids`, `brief_check`, `pr`, `locks`, `tree`
and `contracts` tests.

## 2. Current cost (Gitea API run and job metadata and job logs; no user data quoted)

Job minutes per runner (started to completed; `jobs.py` over `/actions/runs/<id>/jobs`), and the span of each runner class:

| Run | Linux job-min (span) | macOS job-min (span) | Windows job-min (span) |
|---|---|---|---|
| 8554 (train 18, full, green) | 205 (45) | 39 (14) | 55 (26) |
| 8560 (train 19 trial, red) | 136 (38) | 38 (13) | 74 (31) |
| 8509 (train 18, red) | 217 (56) | 40 (15) | 68 (35) |

Biggest host jobs in 8554, from the job logs (nextest `PASS [t]` lines summed per test binary and module):

| Job | Minutes | nextest wall | Test-seconds | Moves to Linux-only (estimate) |
|---|---|---|---|---|
| paddock-dev (windows) | 14.9 | 707 s, 1,871 tests | 3,403 | ~3,100 s (91%): merge_check 1,002, queue 630, golden 493 (already on the precedent branch), pre_push 138, check_* ~270, check_ids 119, tree 116, ci_runs 88, contracts 47, plan 40, generators 35. Kept: ci_host, ci_run, release_isolation, slots, wrapper, load, cli, build-jobs, sccache_eviction (~300 s) |
| crate (windows) | 11.4 | 213 s, 4,813 tests | 1,118 | 418 s (37%) |
| app dyno (windows) | 16.6 | 117 s | 537 | ~100 s (19%); compile 6 min and clippy 3.6 min dominate and stay |
| app spotter / pitwall / pitcrew (windows) | 6.3 / 3.0 / 3.0 | 86 / 26 / 19 s | 248 / 129 / 73 | 120 / 49 / 20 s |
| paddock-dev (macos) | 7.5 | 165 s, 2,285 tests | 1,298 | ~690 s (53%), keeping pre_push, worktree, host steps, wrapper, build-jobs, merge_check, hook, locks on macOS |
| crate (macos) | 12.1 | 114 s | 926 | 179 s (19%) |
| app dyno (macos) | 7.7 | 122 s | 918 | ~230 s (25%) |
| debrief `rust tests (macOS)` (runs 8496, 8547) | 5.6, 11.5 | not parsed | — | about half (18 of 46 crates have no OS code); not measured |

Estimated saving per full ci.yml run (wall of the moved tests at the job's thread count, plus the test binaries no longer
compiled): **Windows about 16 job-minutes of 55 (paddock-dev ~10, crate ~2.5, apps ~3.5)**, and the Windows span from 26 to
about 17 minutes (app dyno becomes the longest Windows job). **macOS about 4.5 job-minutes of 39**, plus 4 to 8 if
`tools-macos` goes (D3) and 3 to 5 per debrief.yml run. A train takes 2 to 3 ci.yml runs (train 18: 8509, 8554), so about
**35 to 50 Windows minutes and 10 to 35 macOS minutes per train**. These are estimates from one run's logs; WP-5 measures them.

## 3. Mechanism

**Common practice (ER-37).** Bazel declares `target_compatible_with` on a target; incompatible targets, and tests that depend on
them, are skipped when a pattern such as `//...` is expanded, and fail only when named explicitly
([bazel.build/extending/platforms](https://bazel.build/extending/platforms), "Skipping incompatible targets"). Buck2 has the same
attribute ([buck2.build, configurations](https://buck2.build/docs/concepts/configurations/), read, not re-fetched today).
rustc's own suite marks tests `//@ only-windows` / `//@ ignore-windows`
([rustc-dev-guide, directives](https://rustc-dev-guide.rust-lang.org/tests/directives.html)). nextest intersects every run with
a profile's default filter (`--ignore-default-filter` turns it off; [nexte.st/docs/running](https://nexte.st/docs/running)) and
selects with filtersets (`package()`, `test()`; [nexte.st/docs/filtersets](https://nexte.st/docs/filtersets)). The common shape:
OS compatibility is declared once, next to the build, and the runner of each platform expands "all compatible tests". Most Rust
projects (tokio, Deno) run the full suite on every OS, which is the cost this note removes; the cheapest form of the Bazel shape in
Cargo is a per-OS package list, which Dyno's `hostRustChecks` already is.

**Proposal.** One declared field per CI contract, read by the planner:

```jsonc
// crates/paddock.ci.json, apps/<app>/paddock.ci.json, and a new tools/paddock-dev/paddock.ci.json (none exists today)
"hostTests": {
  "packages": ["paddock-proc", "paddock-service", ...],          // compiled and tested on the Windows and macOS legs
  "skip":     {"paddock-service": ["model::", "model_linux::"]},  // pure-model modules of a host package: Linux only
  "modules":  ["ci_host::", "ci_isolation::", ...],               // one-crate workspaces (paddock-dev): test-name prefixes
  "only":     {"macos-latest": ["boot_volume::", "pre_push::"]}   // per-OS additions
}
```

`./dev plan --ci` turns it into each host leg's `-p` list (fewer test binaries compiled) and a nextest `-E` filterset, combined
with the existing `since_run` filter (`matrix.filter` in ci.yml's `crate`, `paddock-dev` and `app` jobs; `plan_ci.rs` already
emits one per leg). Default for an unlisted package is Linux only; a missing field keeps today's behaviour (everything), so the
switch is per workspace. Tests stay Rust tests: no `#[ignore]` sprinkled in code.

**The check (no silent loss).** A new conformance check (id from `./dev check new-id --reserve`): every package or module not
in `hostTests` that uses OS-specific code (the cfg forms and OS-API crates above, and the ADR 0109 layer's entry points:
`paddock_proc::command`, `paddock_env::winpath`, `paddock_env::atomic`, `std::process::Command`, file locks) is a finding. For
paddock-dev it also follows `use crate::` edges: a host module importing an unlisted module is a finding (today's
`ci_setup_rust -> local_sccache` and `ci_run -> pre_push::exec` edges are exactly the cases that would otherwise drop the curl
and lock-file tests from Windows). Positive controls: unlist `paddock-proc`, and unlist `local_sccache`, each must go red
(ER-33). Report-only until its first green run on the real tree (ER-41); its allow list only shrinks (ER-46).

**Fit.** ADR 0109 §3 says code above the layer is portable and "the real runners still run it"; this note changes that line to
"the real runners run the layer and the code marked in `hostTests`" (an amendment while 0109 is still proposed). ADR 0113: a
host leg with its OS scope is still complete: `need` is computed from the same `hostTests`, so `ran` covers it; a `since_run`
filter still makes a run partial. The staged branch splits `crate`, `paddock-dev` and `app` into reusable workflows called once
per OS (64d579ae1); the per-OS call passes the planner's package list and filter as inputs, so this lands on top of it or
together with it.

## 4. Risk: past OS-only failures under the new placement

| Failure | Code | Production on that OS? | Still caught there? |
|---|---|---|---|
| 8560 Windows: curl without `SystemRoot` in the S3 health test (fix 1f7bddd77) | `local_sccache` | yes: `ci setup-rust` imports it | yes, via the import edge |
| 8560 Windows: append-mode holder file could not be shortened (fix 5652345ae) | `pre_push::exec` | yes in part: `ci_run` imports it | yes, via the import edge |
| 8560 Windows: `trial.rs` Unix path literals (fix 0b1482f0b) | `ci_runs::trial` | no (coordinator, dev Mac) | no, and nothing ships on Windows: test-only noise |
| 8560 Windows: `pre_push` heavy-queue and `local_sccache` health tests | dev-Mac tools | no | no; noise |
| 8560 macOS and Windows: `check_engine` recorded JS answers | conformance | Linux CI | yes, on Linux |
| 8509 Windows: sccache eviction (491935fce, 18ecd84a4) | `ci_host` | yes | yes |
| 8154 Windows: toolchain links named by PATHEXT (b86f81230) | paddock-testkit | yes (test infra on every OS) | yes |
| 8152 Windows: spawn cost per command (2962f95b5) | paddock-testkit | yes | yes |
| 8147 Windows: planner control in the merge clone (42619e558) | `merge_check` | no (integrator, dev Mac and Linux) | no; no loss |
| 8138 Windows: merge_check loops near the 60 s limit (381137e33) | `merge_check` | no | no; no loss |

Every failure in code that runs on the runner stays caught. The ones that disappear were in code that never runs on Windows, and
cost red rounds there (8560 lost a whole Windows job to them). Residual risk: a portable module later called from a host module
without the table being updated; the import-edge check covers paddock-dev, the package check covers the workspaces.

## 5. Plan

| WP | Work | Agent-days |
|---|---|---|
| WP-1 | `hostTests` schema in the three contract kinds; the planner emits `-p` and the filterset per host leg, merged with `since_run`; ADR 0113 `need`/`ran` read it; goldens | 1.5 |
| WP-2 | Fill the tables: crates (cfg/OS-API audit, the two module skips), the four app legs, debrief's macOS job, paddock-dev modules by host entry points | 1 |
| WP-3 | The check: OS code outside `hostTests`, paddock-dev import edges, positive controls, report-only, shrink-only allow list | 1.5 |
| WP-4 | Wiring: ci.yml host legs (or the staged branch's reusable workflows) and debrief.yml take the inputs; ADR 0109 §3 amendment line | 0.75 |
| WP-5 | One full dispatch; per-OS minutes recorded in docs/benchmarks.md; the check's first green run recorded | 0.5 |
| | **Total** | **5.25** |

Cheaper common-practice option (D1): hand-kept `-p` lists per host leg (as Dyno's `hostRustChecks`) and a hand list of
paddock-dev test prefixes, no check: about 1.5 agent-days, about the same saving, but coverage can drop silently when OS code
moves into an unlisted crate or a host module starts importing a dev-only one (the 8560 curl case).

## 6. Decisions for the user

- **D1.** Full mechanism with the check (5.25 days) or hand lists without it (1.5 days). Recommended: full; it is 3.75 days above
  common practice, so it needs the user's yes (ER-37).
- **D2.** The rule as in section 0 (OS-behaving code only), not the literal "runs there in production" (which keeps nearly all
  crates on Windows and saves little).
- **D3.** `tools-macos` tests tooling for the local test VMs retired on 2026-11-20: delete it (and its scripts if unused), or move
  its `node --test` to Linux. 4 to 8 macOS minutes per run.
- **D4.** Land on top of `ci/staged-fast-then-long` (its per-OS reusable workflows are the natural inputs) or before it on main.
- Later, not costed: split paddock-dev's host part into its own crate, so Windows stops compiling the whole tool (the Windows
  paddock-dev job keeps its compile and clippy, about 3 minutes).
