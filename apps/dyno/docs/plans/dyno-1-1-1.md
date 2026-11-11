# Plan: Dyno 1.1.1

- Status: Plan, 2026-11-08; updated 2026-11-11. Nothing here is built. The five decisions in section 5 were answered by the
  user on 2026-11-08. Decided on 2026-11-11 (memory: `simplify-2026-11-11`, `decisions-2026-11-11`):
  - process ADRs were trimmed after a comparable-practice review (see `docs/adr/trims-2026-11-11.md` once landed): ADR 0072's
    Dyno gate is now only its logging part (ADR 0072 WP-7), and ADR 0033 keeps WP-3, WP-4 and WP-8 (rows A33-3, A33-4, A33-8
    and A72-7 below); ADR 0058 (failed reads retry, WP-1) is trimmed: no six-copy unification;
  - the copy register no longer blocks tags (rule 55(d)); WP-1's `known-copies.toml` entry stays a to-do list item;
  - one review round: one independent review, then fix and land; a second round only for security code or a P1 finding;
  - the Dyno 1.0.0 installers were removed from the release page, with the tag kept (section 6).
  No work package of this plan is dropped; the ADR 0033 and 0072 rows add 5.5-7 days to the totals.
- App: Dyno (`apps/dyno`). Dyno 1.1.0 was tagged on 2026-11-08 (`dyno-v1.1.0`, tag object `01e3548b7`, commit `8efce3fc3`
  on `dyno/1.1.0-rc`). Rule 1 holds: Dyno 1.1.1 runs beside Pitwall 1.1 and Debrief 1.2 within load and CI capacity, and its
  shared changes follow the landing order in section 4.
- Scope: the items deferred from 1.1.0 to 1.1.1 in the coordinator log, the Defender research note, and the
  2026-11-08 elevated-path audit P2s. It is a patch release: no new features beyond what an ADR already promises.
- Estimates: **raw ranges in agent-days**, including tests and review fixes. They are guesses: none of these items has an ADR
  with work packages yet (memory: firm estimates only after a design). Packages that need a design first are marked
  **needs design**; their range is wider and is firmed up once the design exists.
- Sources read: coordinator log `/Volumes/d7sst7/dev-targets/handoff/2026-10-29-overnight.md` (outside the repo; cited as
  `log:<line>`), Defender research note `/Volumes/d7sst7/dev-targets/scratch/defender-research/NOTE.md` (outside the repo;
  cited as `note §n`, recommendations R1-R10), [docs/rules.md](../../../../docs/rules.md), Dyno ADRs
  [0022](../adr/0022-helper-installed-with-the-app.md) and [0027](../adr/0027-helper-for-1-1-0.md), family ADR 0053 (only on
  `origin/docs/dyno-adr-0053-amend1`), the format of [debrief-1-2.md](../../../../docs/plans/debrief-1-2.md) and Dyno's helper
  wiring plan (`origin/docs/dyno-helper-wiring-plan`).
- Rules read: 1, 3, 5, 22, 30, 39, 40, 42, 43, 49, 50, 53, 54, 55, 71.

## 0. Facts this plan starts from (checked 2026-11-08 at the tag `8efce3fc3` unless stated)

| Fact | Evidence |
|---|---|
| The tag is **not on main yet**: `origin/main` is `87c9b8449`, 686 commits behind the rc. A watcher agent lands it. Every package below branches from main **after** that landing. | `git log origin/main..8efce3fc3`; `log:1345` |
| `withRequestStatus` is used by 16 shell stores; the log names about 8 whose event path never clears an error after a failed first read (activity, settings, service-health, notifications, channels, mcp, diagnostics, updates). The Jobs store already has its own retry on `JOBS_CLOCK` (`fix/dyno-jobs-retry`, merged in the tag). | `git grep withRequestStatus ui/projects/paddock-ui/shell`; `log:1316`, `log:1336-1339` |
| The jobs snapshot carries no agent instance identity, so a reconnected app cannot tell a job of the previous agent from a live one. | `crates/paddock-jobs/src/tauri.rs:128` (`jobs_snapshot`); no instance or epoch field in `crates/paddock-jobs/src` |
| The installer include on the tag is v4 (level 44 = `TOOL_MISSING`). The Pitwall branch's include goes to v7 and uses level 44 for `UNTRUSTED`; the merge must renumber `UNTRUSTED` to 45. | `crates/paddock-setup/nsis/paddock-per-machine.nsh:55`; `log:1322` |
| The Dyno side of Pitwall's B6c (include v7, the copied-uninstaller guard for Dyno's `dyno-agent.exe` / `dyno-helper.exe` hooks) lands only with the Pitwall landing, and needs a Dyno installer build and per-machine VM rows (rule 54). | `log:1304`, `log:1307`, `log:1309` |
| Defender convicted `paddock-setup.exe` (`Trojan:Win32/Bearfoos.A!ml`) once in six installs of the 0ad build; the installer exited 0 with a crippled install. Two builds differ in 14 bytes (timestamps and PDB GUID): every build has a new hash. | `log:1317`; note §1 |
| `paddock-setup.exe` has a thin version resource (no CompanyName, LegalCopyright, OriginalFilename, InternalName) and no manifest; it is the only binary in the installer that enables `SeDebugPrivilege`. No `/Brepro` or `--remap-path-prefix` anywhere. | note §2; `crates/paddock-setup/src/versionres.rs` |
| ADR 0022 §3 promises "An update is already running." for a second `Update` request to the helper. The helper wire has **no `Update` request at all**: updates go through `paddock-setup` (ADR 0053), whose busy refusal reads "Another install of this app is already running." | `0022-helper-installed-with-the-app.md:451-453`; no `UpdateRequest` in `dyno-core/src/helper_wire.rs`; `crates/paddock-setup/src/apply.rs:124` |
| The macOS helper's build is compared only when the installed helper's version equals the app's, so an older helper on the same API shows Ready and the newer bundled one is never offered (ADR 0027 §4 says it is offered). | `dyno-agent/src/helper_link.rs:281-283` |
| The discovery guard waits in 500 ms slices and adds the nominal slice, so the 30 s period drifts (~31.6 s at utility QoS). | `dyno-agent/src/build_id.rs:146-157`; `log:1221` |
| Linux `processes()` keeps every entry of sysinfo's table; only `details()` filters `thread_kind()`. `details()` refreshes one pid `with_tasks()`, which can put that pid's threads into the table the next tick reads (inference; needs the failing test). | `dyno-platform/src/processes.rs:294-326` vs `:343` |
| Dyno sets `--pd-button-height: var(--control-h)` (28 px Standard, 24 px Compact); the family token is 32 px Standard, 28 px Compact. | `apps/dyno/app/src/styles.scss:709`; `ui/projects/paddock-ui/scss/_tokens.scss:107-108,133-134` |
| The page layout fix `fix/dyno-page-layout-1.1.1` is already in the tag (approved at `b15d93619`). What is left is the re-review's P2 list. | `git merge-base --is-ancestor`; `log:1193` |
| ADR 0053 is cited by the rc's installer hooks but exists only on `origin/docs/dyno-adr-0053-amend1` (5 commits not in the tag). | `log:1223`; `git ls-tree origin/docs/dyno-adr-0053-amend1` |
| `a6b906bbf` ("link probes the agent lock at one check") is only on `pitwall/1.1-b1*` branches; Dyno 1.1.0 keeps the ~400 ms held-lock probe, and `docs/lessons.md` wrongly says the wrapper is gone. | `git branch -r --contains a6b906bbf`; `log:1235` |
| The Linux AppImage step downloads linuxdeploy and AppRun from GitHub at bundle time; a 504 on 2026-11-08 failed dyno-release 7693 and 7694, with no `::error::` annotation. | `.gitea/actions/tauri-bundle/action.yml`, `.gitea/workflows/app-release.yml:138`; `log:1340-1344` |
| The helper wiring checks (ADR 0052) are `#[ignore]`d and report: `SocketOwners` and `FileHolders` advertised but never sent; `LogTail` undeclared; the user-guide and CHANGELOG helper-readings markers missing; the `WolState` `[[unwired]]` entry stale. | `log:1247` |

## 1. Order at a glance

Ranked by user impact and risk, then by what unblocks what. Waves are an order, not a barrier: a package starts when its
dependencies are merged and a slot is free.

| Wave | Packages | Why here |
|---|---|---|
| 0 | Dyno 1.1.0 lands on main (watcher agent, not part of this plan) | every branch below starts from it |
| 1 | **WP-1** shared failed-read retry; **WP-2** release-path robustness (AppImage pin, `::error::`); **WP-3** exit-143 flakes; **WP-4** Defender evidence and per-release submission | WP-1 is user-approved as the first shared item after the tag and must land **before** the Pitwall landing; WP-2 and WP-3 protect every later CI and release run; WP-4 is harness-only and its evidence answers decision 5 |
| - | Pitwall 1.1 landing (not part of this plan) brings include v7, the Dyno uninstall-hook guard, the shared sidecars and tray footer | WP-5, WP-6 (`/DEPENDENTLOADFLAG`) and WP-7 change `crates/paddock-setup`, which B6c rewrites; starting them earlier means a hard merge |
| 2 | **WP-5** Windows installer; **WP-6** security P2s and ADR 0053; **WP-8** jobs across agent restarts (needs design) | WP-5 is the highest user impact on Windows (a crippled install that says it succeeded); WP-6 lands ADR 0053 that WP-5 and WP-7 cite; WP-8 follows WP-1 (same store files) |
| 2 (parallel) | **WP-9** macOS helper and guard; **WP-10** Linux threads; **WP-11** `dyno mcp` delay; **WP-12** wiring checks in CI; **WP-13** link-probe decision | independent files, any free slot |
| 3 | **WP-7** byte-stable, described `paddock-setup`; **WP-14** UI; **WP-15** row 41 | WP-7 after WP-5 and WP-6 (same crate, one owner at a time) and after decision 4; WP-14 after decisions 1 and 2 so screenshots are taken once; WP-15 after decision 3 |
| 3 (conditional) | **WP-16** Restart Manager first | only if decision 5 says 1.1.1, or WP-4's evidence shows a behaviour-origin detection |
| 4 | **WP-R** release: rule 54 installer build, VM rounds per OS, Defender evidence run, notes, tag | last |

## 2. Work packages

Conventions for every package:

- One agent, its own sparse worktree (`git sparse-checkout set --cone crates ui scripts docs tools apps/dyno`), its own
  `CARGO_TARGET_DIR`, its own e2e port, `CARGO_BUILD_JOBS=2`, heavy steps through `./dev plock` (release lane for Dyno work).
  Branch `dyno-1-1-1/<wp>` from main after wave 0; the coordinator merges into `integrate/dyno-1.1.1`.
- A bug fix starts from its failing test (rule 5); a flaky test is made deterministic (rule 49, ER-12). Affected tests only
  (rule 3), Chromium only for Playwright. A mutation check over the package's range before landing (redesigns 2026-11-05).
- Shared code (`crates/`, `ui/`, `.gitea/`) is changed with its adoption by every app in the same change (rules 39, 42, 43);
  `.gitea/` and other integration points by the integrating agent or by one declared line.
- A change to Dyno's release path (installer, `paddock-setup`, bundling) builds Dyno's installers before it lands (rule 54).
- Every screenshot or drawing gets an independent review (rule 22). A VM row counts only if its precondition was verified and
  its probe showed it can see the thing (rule 71).

Roles: **implementer** = Sonnet; **designer**, **reviewer**, **researcher** = Opus; **scout** = Haiku. Every package is reviewed
by an agent that did not write it.

### Wave 1

| # | Package | Owned files | Role | Depends on | Acceptance | Raw days | State |
|---|---|---|---|---|---|---|---|
| WP-1 | **Shared failed-read retry** (ER-21 / rule 40, user OK `log:1339`), built to the existing [ADR 0058](../../../../docs/adr/0058-failed-reads-retry.md) as trimmed on 2026-11-11 (no new ADR, no six-copy `BackoffPolicy` unification). The pattern (a store's event path never clears `requestStatus` after a failed first read) and the mechanism: `withRequestStatus` owns "error, then a later success" (a snapshot clears the error, a delta triggers a re-read, a backoff retry runs while in error). Move the Jobs store's retry there; adopt it in the ~8 affected stores; one spec that runs over **every** store using `withRequestStatus` (16 today). Includes the Jobs spec gaps: stop on success after an event re-read (mutant M3 survives today), no re-set of an identical error, the two-microtask wait, the rate-limit doc line (`log:1320`, `log:1338`). The Rust backoffs in `paddock-ipc` `link/client.rs` and `paddock-jobs` `schedule` are not unified (ADR 0058 trim); at most a `known-copies.toml` entry (rule 55). | `ui/projects/paddock-ui/shell/store/src/request-status.ts`, the stores' `*.store.ts` and specs under `ui/projects/paddock-ui/shell/*/src/`, `docs/known-copies.toml` (one entry if needed) | implementer | wave 0 | the every-store spec fails on today's code for each named store and passes after; a store added later without the feature fails the spec; demos and Dyno affected e2e green | 3-5 | done (`feat/shared-read-retry` in the train: `eff75affc`, `c679b03d6`) |
| WP-2 | **Release-path robustness.** Pin linuxdeploy and AppRun by version and sha256 and cache them (actions/cache on the user's cache server, or the CI image), so a GitHub outage cannot fail a bundle; every failing download or bundle step prints one `::error::` with the URL, status and next step (ER-27). | `.gitea/actions/tauri-bundle/action.yml` (integrating agent), the script it calls | implementer | wave 0 | a run with the cache warm and GitHub blocked (deny the host in a local act run) bundles the AppImage; a forced 504 prints the annotation; hash mismatch fails loudly; dyno-release dispatched with bundles (rule 54). **Run evidence still owed** (branch `fix/dyno-wp2-appimage-pins`, script-level tests only): train 6's rule-54 dyno-release dispatch with bundles, on its second run, shows "from the cache" for both tools, no Tauri download line for linuxdeploy or AppRun, and an AppImage that starts; the GitHub-blocked act run waits for Docker (ER-14) | 1-2 | open (pins and `::error::` lines in `fix/adr0085-minimum`, which contains `fix/dyno-wp2-appimage-pins`; the run evidence is owed) |
| WP-3 | **dyno-agent exit-143 flakes**: `the_agents_start_and_stop_are_in_its_own_dated_log_with_millisecond_counts` and `term_stops_a_running_agent_cleanly_and_removes_its_discovery_file`. Hypothesis (`log:1259`): the test waits on the discovery file, which `run()` writes before RUNNING, so a TERM in that gap takes the EndNow path. Bug-fix skill: list causes, reproduce under load, fix by waiting on RUNNING. | `apps/dyno/crates/dyno-agent/tests/` (the two tests and their helper) | researcher (cause) + implementer | wave 0 | ER-28: the reproduction fails with the gap and stays green without it, at the measured failure rate; no sleeps, no raised timeouts | 1-2 | done (`fix/dyno-term-ready` `9eecbfa17`, `fix/dyno-agent-term-race` `90845135c`, both in the train) |
| WP-4 | **Defender evidence and submission** (note R1, R2; WP1, WP2 there). The Windows install row keeps the full 1116/1117 event text, `Get-MpThreatDetection` (InitialDetectionTime, ProcessName, DetectionSourceTypeID), `Get-MpComputerStatus` versions, and the `MPLog-*.log` lines around the detection; prints one "Defender: origin / type / source / process" line; can repeat the install row N times per snapshot for a rate. Release checklist step: submit `paddock-setup.exe` and the installer as a software developer after each tag and record the submission id and determination. Also check VirusTotal's "obfuscated" and "detect-debug-environment" tags (note addendum). | `scripts/vm-smoke/` Windows rows, `apps/dyno/docs/releasing.md`, the release skill's checklist text (one line) | implementer | wave 0 | a control run that drops the EICAR test file in the guest shows every field in full (rule 71) before a "no detection" counts; 10 runs per snapshot on the 1.1.0 installer recorded as a baseline rate | 1.5-2.5 | partly: the Defender evidence run and the submission steps (`24a9b43d7`, `ffbf00e89`, `releasing.md`) are in the train; left: the rate run on the final installer (Windows, with WP-R) |

### Wave 2 (after the Pitwall landing for WP-5 and WP-6)

| # | Package | Owned files | Role | Depends on | Acceptance | Raw days | State |
|---|---|---|---|---|---|---|---|
| WP-5 | **Windows installer.** (a) A silent `/UPDATE` that aborts at level 44 restarts the app and writes a `setup.log` line from NSIS. (b) POSTINSTALL records a missing tool once and then aborts once, with an "installed, but ..." message, never exit 0 on a crippled install. (c) The status says "paddock-setup.exe missing", not "out of date" (`log:1289`). (d) Harness case M4 does a real uninstall (`EXIT_SAME_FOLDER=1` plus `HELPER_UNINSTALL`). (e) Adopt include v7 and verify the Dyno uninstall-hook guard that arrive with the Pitwall landing (renumbered `UNTRUSTED` = 45). | `crates/paddock-setup/nsis/`, `crates/paddock-setup/src/` (status text), `apps/dyno/app/src-tauri` NSIS hooks, `apps/dyno` per-machine status mapping, `scripts/vm-smoke/` M4 | implementer (after a designer reads (a)-(b) against ADR 0053) | Pitwall landing, WP-6's ADR 0053 landing | failing tests first for (a)-(c) (NSIS harness and Rust); VM rows on a Windows VM: silent update with the tool removed, normal install, M4 uninstall, per-machine rows of B6c's Dyno side; Dyno installers built (rule 54) | 4-7 | partly: the code is in the train (`b5a5a0bd2` installed-but message at level 44, `0f19d2aa0`, include v10 with `UNTRUSTED` = 45, harness hook `HELPER_UNINSTALL`); left: the Windows VM rows and a Dyno installer build (rule 54) |
| WP-6 | **Security audit P2s** (`log:1253-1257`). (1) `remove_data_folder` (`dyno-helper/src/setup.rs:386`) calls `verify_container` and `verify_chain` before deleting, with a FakePort junction test. (2) `paddock-setup.exe` links with `/DEPENDENTLOADFLAG:0x800` (it runs from `$PLUGINSDIR` and imports `version.dll`). (3) `helper_counters.rs` pins `$PSHOME\Modules` for the PowerShell it runs as SYSTEM (or reads the counters through WMI from Rust). (4) Land ADR 0053 from `docs/dyno-adr-0053-amend1` and fix §2.5: scope it to the installer and updater, and add that a service data root under `%ProgramData%` goes through `privileged::trust`. | `apps/dyno/crates/dyno-helper/src/setup.rs`, `crates/paddock-setup/build.rs` (link arg only), `apps/dyno/crates/dyno-platform/src/helper_counters.rs`, `docs/adr/0053-per-machine-windows-installs.md` | implementer + reviewer (security) | Pitwall landing for (2) and (4) (B6c also edits `paddock-setup` and the ADR's adoption rows) | junction test fails before (1); a PE check in the release gate asserts the load flag; a test asserts the pinned module path; ADR reviewed independently; Windows test job and installer build (rule 54) | 3-5 | partly: `feat/dyno-wp6-setup-verify` (`85708c30c`, `cc59c905a`: data-folder removal check, system32 DLL load, `PSModulePath`, ADR 0053) is not in the train yet; left: the Windows VM check |
| WP-8 | **Jobs across agent restarts** (**needs design**). (a) A job of a previous agent instance shows as ended ("lost when the agent restarted"), not running, and its Cancel is gone; today the list keeps it and Cancel returns `AgentUnavailable` (`log:1331`). (b) A reading page (Extras > Sleep) that holds a job id from before a restart starts a fresh read instead of "That job has no result ..." (`log:1342`). (c) Each Cancel button is named "Cancel <job title>" (`log:1352`). Design question: does the instance identity live in `paddock-jobs`' snapshot (shared: Pitwall and Spotter get it too) or only on Dyno's agent link? | design note in `apps/dyno/docs/design/`, then `crates/paddock-jobs/src/` or `apps/dyno/crates/dyno-agent` link, `ui/projects/paddock-ui/shell/jobs/src/`, the Dyno reading-page stores | designer (note) + implementer | WP-1 (same store files) | failing tests for (a)-(c) first (store spec with a restarted fake agent; Rust test for the snapshot identity); axe shows distinct Cancel names; macOS and Windows VM row: restart the agent during a job | 3-6 | open (needs design; no commit) |

### Wave 2, in parallel (independent files)

| # | Package | Owned files | Role | Depends on | Acceptance | Raw days | State |
|---|---|---|---|---|---|---|---|
| WP-9 | **macOS helper across versions and the guard drift.** (a) An older helper on the same `HELPER_API` is offered the newer bundled one (ADR 0027 §4), never a downgrade (`helper_link.rs:281`). (b) `wait_or_stop` uses an `Instant` deadline, which also tightens the binary watcher (`build_id.rs:146`). | `apps/dyno/crates/dyno-agent/src/helper_link.rs`, `apps/dyno/crates/dyno-agent/src/build_id.rs`, `dyno-core/src/helper_wire.rs` (`judge_build` only if needed) | implementer | wave 0 | failing unit tests first (older version, same API → offer; newer → none); guard period within a slice of 30 s with an injected clock; macOS VM row with a 1.1.0 helper installed and a 1.1.1 app | 2-3 | done (`fix/dyno-1-1-1-wp9-wp10` in the train: `f6d5fd3a9`, `252c4895b`, `b999b326c`) |
| WP-10 | **Linux threads counted as processes.** Filter `thread_kind()` in `processes()` as `details()` does, so group memory is not inflated. | `apps/dyno/crates/dyno-platform/src/processes.rs` | implementer | wave 0 | a failing test first (a real child with several threads, Linux only, or the fake table); Linux VM: group memory of a many-threaded process matches `smaps_rollup` (rule 26) | 1-2 | done (`fix/dyno-1-1-1-wp9-wp10` in the train: `a1fd6e9f1`); the Linux VM row against `smaps_rollup` is not run |
| WP-11 | **`dyno mcp` waits ~11 s after the app was killed** before "The app isn't open" (`log:1342`): the connect waits on a dead listener. Fail fast when the discovery file's owner is gone (rule 53: verify the pid with its start time before waiting). | `apps/dyno/crates/dyno-agent/src/cli.rs` or the MCP bridge's connect, `crates/paddock-mcp` only if the wait is there | implementer | wave 0 | test: a killed owner with a stale discovery file answers in under 1 s; a live but slow owner is still waited for | 1-2 | done (`fix/dyno-mcp-dead-owner`, `b517decb9`) |
| WP-12 | **Wiring checks run in CI** (Dyno's part of the ER-21 candidate "every check runs somewhere", `log:1296`). Remove the `#[ignore]` from the helper wiring checks, add them to a CI job, and fix their findings: send or stop advertising `SocketOwners` and `FileHolders`; declare `LogTail`; add the user-guide and CHANGELOG helper-readings markers; drop the stale `WolState` `[[unwired]]` entry. The family meta-check (every check id is gated or registered as not gated) is a separate ADR, not in 1.1.1. | `apps/dyno/crates/*/tests/helper_wiring*`, `apps/dyno/docs/user-guide.md` markers, `apps/dyno/CHANGELOG.md` markers, `apps/dyno/paddock.ci.json` (checks list) | implementer | wave 0 | the checks fail on today's tree in CI, then pass; the wired readings have their contract tests (wiring plan §3) or are removed from the advertised list with a CHANGELOG line | 2-5 | done (`488592340`: `fix/dyno-wiring-checks-in-ci`) |
| WP-13 | **Link-probe decision (`a6b906bbf`).** Decide whether Dyno's link takes the one-check lock probe from Pitwall's branch (removes the ~400 ms held-lock probe) or keeps it; either way correct `docs/lessons.md`. Default: take it with the Pitwall landing, since it is the CI 7257 fix and the code is shared. | `apps/dyno/app/src-tauri/src/link/`, `docs/lessons.md` (one entry) | implementer | Pitwall landing if taken | the link tests (including `a_stale_discovery_file_beside_a_slow_lock_holder...`) pass on Windows, macOS and Linux CI; lessons text matches the code | 0.5-2 | done (`a6b906bbf` in the train; `docs/lessons.md:453` says the probe is one check) |

### Wave 2, ADR packages kept by the 2026-11-11 trims

Days are taken from each ADR's own work-package table.

| # | Package | Owned files | Role | Depends on | Acceptance | Raw days | State |
|---|---|---|---|---|---|---|---|
| A33-3 | **Dyno ADR 0033 WP-3: Windows facts** (`win_os_name` first, then the registry, power, WMI and folder reads, with per-fact logging), as in [ADR 0033](../adr/0033-no-test-fake-in-a-release-build.md) | as ADR 0033 WP-3 names | implementer | ADR 0033 WP-0, WP-1 | ADR 0033 WP-3's acceptance | 1.5-2 | open (no `win_os_name` or Windows fact reader in the code) |
| A33-4 | **Dyno ADR 0033 WP-4: startup from the registry and folders** (Run keys, Startup folders, StartupApproved; the deferred sources say "not read here") | as ADR 0033 WP-4 names | implementer | ADR 0033 WP-0, WP-1 | ADR 0033 WP-4's acceptance | 2.5-3 | open |
| A33-8 | **Dyno ADR 0033 WP-8: docs** (architecture, features, user guide, CHANGELOG 1.1.1, lessons entry) for what A33-3 and A33-4 build | as ADR 0033 WP-8 names | implementer | A33-3, A33-4 | ADR 0033 WP-8's acceptance | 0.5-1 | open (waits for A33-3 and A33-4) |
| A72-7 | **ADR 0072 WP-7: connection-end logging** (ER-27 end lines, summaries, rigs print the product log on failure); Dyno's tag gate since the 2026-11-11 trim of [ADR 0072](../../../../docs/adr/0072-every-connection-end-says-why-in-order.md) | as ADR 0072 WP-7 names | implementer | wave 0 | ADR 0072 WP-7's acceptance; gates the Dyno 1.1.1 tag | 1 | partly: `feat/adr0072-wp7-end-lines` (`16aaf2445`, `paddock-ipc` and `paddock-mcp` end lines) is not in the train; left: the rest of the Dyno end lines and rigs |

### Wave 3

| # | Package | Owned files | Role | Depends on | Acceptance | Raw days | State |
|---|---|---|---|---|---|---|---|
| WP-7 | **Byte-stable, described `paddock-setup.exe`** (note R3, R5). Deterministic link (`/Brepro` or a fixed `/timestamp:`) and `--remap-path-prefix` for the cargo registry and source paths, scoped to the `paddock-setup` build (its `build.rs` link args and `scripts/stage-paddock-setup.mjs`), not the global `.cargo/config.toml`; a CI check that builds it twice and compares sha256 (also measure the PDB GUID). If decision 4 is yes: its own version, `check-version` and the release gate compare against it. Full version resource (CompanyName, LegalCopyright, OriginalFilename, InternalName) and an `asInvoker` manifest with `supportedOS`. | `crates/paddock-setup/build.rs`, `crates/paddock-setup/src/versionres.rs`, `scripts/stage-paddock-setup.mjs`, `conformance/release/check-per-machine-installer.mjs` | implementer | WP-5, WP-6 (same crate), decision 4 | two builds of the same commit give the same sha256 on the Windows runner; the shipped binary no longer contains the build account's path; the gate asserts every resource field and the manifest; Dyno installers built (rule 54); Pitwall adopts in the same change | 2-3; +1-2 for its own version | partly: `31fd0b146` in the train (own version, full version resource, manifest, release gate, `/Brepro`); left: the same-sha256 proof on the Windows runner, the Dyno installer build and Pitwall adopting it |
| WP-14 | **UI.** (a) The menu-bar "Network up" item shows only the up rate (`log:1192`). (b) The CPU card's Metric select works (`log:1128`). (c) The Processes page shows a hint ("Other accounts appear after the helper's first read") instead of "—" before the first helper sweep (`log:1290`). (d) macOS Disk activity "Not available": find out whether it is expected; if so, the card says why, if not, fix it. (e) The layout re-review P2s (`log:1193`: Extras tab width jump, Listening buttons wrapping at 1280, Connections empty band at 1920, Firewall rows and button at 1280, Alarms columns, Processes subtitle wrap). (f) Decisions 1 and 2 applied. | `apps/dyno/app/src/app/features/` (tray, dashboard CPU card, processes, extras, network, firewall, alarms), `apps/dyno/app/src/styles.scss` (decision 2), page-layout spec | designer (layout) + implementer | decisions 1 and 2 | failing spec per bug first; page-layout spec at 1024, 1280, 1440, 1920 in both themes; one screenshot set retaken and reviewed independently (rule 22) | 4-7 | partly: `feat/dyno-wp14-wp15` (`83128792d`) is in review fixes and not in the train |
| WP-15 | **Row 41** (decision 3). Recommended: amend ADR 0022 §3 to say the helper takes no updates; updates go through `paddock-setup` (ADR 0053) and a second install is refused with "Another install of this app is already running."; record what macOS and Linux do; rewrite row 41 to measure that. If the user chooses "build": an updater lock and state file with the promised text on all three OS. | `apps/dyno/docs/adr/0022-helper-installed-with-the-app.md` (amendment), the VM row text; or, if built, the updater code (needs design) | designer | decision 3 | amendment reviewed independently and row 41 measurable on the Windows VM; or, if built, failing test and VM rows per OS | amend 0.5-1; build 3-6 (needs design) | partly: `feat/dyno-wp14-wp15` (`c8d30ef3a`, ADR 0022 amendment) is in review fixes and not in the train; left: the VM row text |
| WP-16 | **Restart Manager first** (note R4; conditional on decision 5). `stop-all` asks Restart Manager first and enables `SeDebugPrivilege` and kills by handle only when RM lists a process in another session or RM shutdown fails. ADR 0053 §3 amendment, fake-machine tests for both branches, VM rows 12-14. | `crates/paddock-proc/src/installed.rs`, `installed/native.rs`, ADR 0053 §3 | designer + implementer | WP-6 (ADR landed), WP-4 evidence | both branches tested on a fake machine; VM rows 12-14 pass; Defender evidence run of WP-R repeated | 3-5 | open (conditional on the WP-4 evidence, decision 5) |
| WP-17 | **Pin linuxdeploy-plugin-appimage** (done with ADR 0085 WP-9, 2026-11-12). Tauri downloaded it unverified from linuxdeploy's moving `continuous` release. The project has no stable release, so the user chose the dated build `1-alpha-20250213-1`: `conformance/ci/tauri-tools.mjs` pins it by URL and SHA-256 and puts it in Tauri's tools folder like linuxdeploy and AppRun; a tag fails on any unpinned tool. | `conformance/ci/tauri-tools.mjs`, its test | integrator | WP-2 | a bundle log shows no download of the plugin; a hash mismatch fails loudly | – | done |

### Wave 4

| # | Package | Owned files | Role | Depends on | Acceptance | Raw days | State |
|---|---|---|---|---|---|---|---|
| WP-R | **Release 1.1.1**: version bump and regenerated files, release notes, dyno-release dispatched with gates and bundles on the candidate (rule 54), VM rounds per OS (install, update from 1.1.0, per-machine rows, jobs-restart row, helper across versions), the Defender evidence run (WP-4) N times on the final installer, the FP submission (WP-4 step), tag from CI only. | `apps/dyno/CHANGELOG.md`, version files, release log | implementer + scout (CI watch) + VM agents | all chosen packages | full suite once on the combined tree (ER-8); every VM row PASS or recorded `not measured` with a reason (rule 71) | 3-5 | open |

### WP-14 status (2026-11-12)

- (c) Processes: before the helper's first read the engine sends a hint (`LiveVia.processesHint`), not an unavailable reading; a worker that
  cannot start ends the wait with its reason. The page shows the hint through the reading source's hint line.
- (d) macOS Disk activity "Not available": **root cause open.** The branch makes the card say the platform's own reason, and on the
  development Mac the probe measures disk busy time, so the failure was not reproduced there. The VM round, or the user on the Mac that showed it,
  reads which reason now shows; the cause is recorded from that.
- (e) Listening toolbar: one row at 1280 and 1240 (test `layout-p2.spec.ts`, red first at 1240 on the old layout). **Deferred: Firewall rows at 1280** (about 11 rows): the
  table's height is what the window leaves after the pf card, and the last row is cut by the scroller's edge; no condition for "cut" was agreed, so no
  test could fail first. Take it up with the layout designer before the screenshot review. The "State at most 200 px" check on Alarms was vacuous
  (the column is a fixed 190 px) and is dropped.

## 3. Totals (raw ranges, agent-days)

| Group | Packages | Raw days |
|---|---|---|
| Shared retry and jobs | WP-1, WP-8 | 6-11 |
| Windows installer, security, Defender | WP-4, WP-5, WP-6, WP-7 | 10.5-17.5 (+1-2 own version) |
| macOS, Linux, CLI | WP-9, WP-10, WP-11 | 4-7 |
| Tests and CI | WP-2, WP-3, WP-12, WP-13 | 4.5-11 |
| UI and row 41 (amend) | WP-14, WP-15 | 4.5-8 |
| ADR packages kept by the trims | A33-3, A33-4, A33-8, A72-7 | 5.5-7 |
| Release | WP-R | 3-5 |
| **Total, recommended options** (decision 3 amend, decision 4 yes, decision 5 after the evidence) | | **39-68.5** |
| If row 41 is built instead | WP-15 | +2.5-5 |
| If Restart Manager first is in 1.1.1 | WP-16 | +3-5 |

With three builder lanes (the current allocation beside Pitwall and Debrief), the critical path is wave 0, then WP-1, then the
Pitwall landing, then WP-6, WP-5, WP-7 in sequence on `paddock-setup`, then WP-R: about 16-27 agent-days on that path (WP-7 with its own version), so
roughly 3-6 weeks elapsed plus any wait for the Pitwall landing. **This is a guess**: it depends on that landing date, and WP-8 and any "build" choices have no
design yet. Firm numbers follow once WP-8's design note has work packages.

## 4. Dependencies and landing order

1. Dyno 1.1.0 lands on main (wave 0).
2. WP-1 (shared retry) lands **before the Pitwall landing** (user, `log:1339`); WP-2, WP-3, WP-4 land when ready.
3. Pitwall 1.1 lands. It brings include v7 (renumber `UNTRUSTED` to 45 on the merge), the Dyno side of B6c (uninstall-hook
   guard, per-machine updater wiring), the shared sidecars (`fix/dyno-sidecars-shared`) and tray footer
   (`feat/tray-popover-footer`), and, if WP-13 takes it, `a6b906bbf`. That landing itself builds Dyno's installers (rule 54).
4. On `crates/paddock-setup` one owner at a time: WP-6 (ADR 0053 and the load flag) → WP-5 → WP-7 → WP-16 if chosen.
5. WP-8 after WP-1 (both touch the jobs store). WP-14 after decisions 1 and 2. WP-15 after decision 3.
6. WP-R last.

## 5. Decisions for the user

| # | Question | Recommendation | Why | State |
|---|---|---|---|---|
| 1 | Should the Processes table's **Account** column survive at a 1024 px window with the sidebar open? | **Yes: drop Disk first**, then lower the other minimums; Account goes last. | "Processes of every account" is a 1.1.0 feature, and the account is what tells a user whose process it is; Disk is also shown in the details pane. | decided (user, 2026-11-08): keep Account at 1024 px, drop Disk first (memory `dyno-release-asap.md`). |
| 2 | Should Dyno's buttons match the **family's 32 px** (Compact 28 px) instead of Dyno's 28/24? | **Yes**, removing the override at `styles.scss:709`, done inside WP-14 so the layout is checked and screenshots are retaken once. | The shared tray footer already shows the mismatch beside Pitwall and Spotter; one height across the family; 28 px stays available as Compact. | decided (user, 2026-11-08): buttons match the family 32/28 px (memory `dyno-release-asap.md`). |
| 3 | **Row 41**: build "An update is already running." as ADR 0022 §3 describes, or amend the ADR? | **Amend the ADR** (WP-15). | The helper has no `Update` request; updates go through `paddock-setup`, which already refuses a second install with its own text. Building a helper update path adds an elevated, file-receiving surface for no user need. | decided (user, 2026-11-08): amend ADR 0022 (memory `dyno-release-asap.md`); WP-15 takes the amend branch. |
| 4 | Should **`paddock-setup` get its own version**, so its hash stays stable across app releases? | **Yes** (WP-7). | Its bytes change with every app version today only because the app version is baked in; a stable hash keeps a Microsoft "not malware" determination and any prevalence counting. The per-app identity values stay baked in, so the hash is stable per app, not across apps. Pitwall ships it too after B6c. | decided (user, 2026-11-08): `paddock-setup` gets its own version (memory `dyno-release-asap.md`). |
| 5 | **Restart Manager first** in 1.1.1, or after the evidence? | **After the evidence**: run WP-4 first; do WP-16 in 1.1.1 only if the captured Detection Source shows a behaviour origin, otherwise in 1.2. | The link between the privileged kill path and this verdict rests on one detection (n = 1); WP-16 is an ADR amendment plus VM rows 12-14 again. | decided (user, 2026-11-08): only if the 1.1.1 Defender evidence points to behaviour (memory `dyno-release-asap.md`). |

Still standing from earlier: signing stays off (2026-11-05); if detections recur after WP-4, WP-5 and WP-7, the note's R7
(signing) comes back to the user.

## 6. On the log but not in this plan

Kept here so they are not lost; each needs the user or the coordinator to place it.

- Harness gaps: 24m (second account), the update flow rows 24d/24e, rows 42/43 in VMs (`log:1192`).
- A job-level warning outcome for optional database failures; mount and wake OS hooks for disk refresh; `/var/log/dyno-helper`
  after Turn off and the remove question (`log:1192`).
- Decimal storage units (the Finder figure is in the tag, `dyno-platform/src/volumes.rs`; check the units).
- 64 dyno-agent tests that leak on Windows (ER-14 look, `log:1181`).
- The family ER-21 candidates found during 1.1.0 (dialog commands vs contract, undeclared cross-inputs, EINTR on timed socket
  reads, red intermediate commits, the conformance meta-check): family ADRs, not Dyno 1.1.1 packages.
- Done: the 1.0.0 installers were removed from the release page (user OK, 2026-11-11), and its `latest.json` was deleted
  too; the `dyno-v1.0.0` tag is kept.
