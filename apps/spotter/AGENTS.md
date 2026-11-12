# Spotter — engineering rules for coding agents and contributors

Spotter watches for new releases anywhere (folders and shares, HTTP listings, JSON APIs, forges, feeds, web pages
including JavaScript-rendered ones) and tells you the moment one appears: a Rust core (`crates/spotter-core`), source
adapters (`crates/spotter-sources`), the engine (`crates/spotter-engine`), the renderer (`crates/spotter-render`), a
Tauri 2 shell (`app/src-tauri`) and an Angular UI (`app/src`). It is part of the Paddock suite and is local only: no
cloud, no telemetry. See README.md, `docs/adr/` and `docs/suite-lessons.md` (follow every lesson in it).

<!-- paddock:block rules rev=7ad91a44 -->
## Paddock rules (binding)

The family's binding rules, copied from [paddock/docs/rules.md](https://dev.decryptology.net/decryp7/paddock/src/branch/main/docs/rules.md) (the source; update there first). Where an older rule in this file disagrees, these win.

### Way of working
1. **One release at a time.** Release-critical work comes first. Other apps may progress in parallel only while load and
   CI capacity allow, and only if they don't change shared code (`crates/`, `ui/`) the release depends on; tags stay
   serialized (amended twice on 2026-10-28; ER-1).
2. **Every agent works in its own git worktree** with its own `CARGO_TARGET_DIR` and its own e2e port; the main checkout
   stays on `main` for the coordinator.
3. **Tests in a branch: only the affected ones** (touched crates, touched specs, related Playwright files), with
   Playwright in Chromium only. The full suite, Chromium and WebKit, runs once on the combined result before a local
   build or release, plus CI. Fix everything locally first; CI confirms, it is not where failures are found.
4. **When CI fails:** fix and re-run only the failed tests locally, push once; never push fix by fix to see what CI says.
   Lint and type-check the touched files before every push.
5. **Every bug fix starts with a failing regression test.** A test-only failure is fixed properly (deterministic, no
   sleeps), never silently skipped.
   A reviewer of a bug fix runs a mutation check on it (user, 2026-10-30), in their own worktree: for Rust,
   `cargo mutants --in-diff <diff> -p <crate> --timeout-multiplier 3` with `CARGO_BUILD_JOBS=2` (install once with
   `cargo install cargo-mutants --locked`); for TypeScript, revert the fix and run the new test. The review reports whether
   the new test fails without the fix; a test that still passes proves nothing and is sent back. (A `cargo mutants
   --in-diff` CI job on the shared crates comes after the releases; see
   [lessons.md](https://dev.decryptology.net/decryp7/paddock/src/branch/main/docs/lessons.md#a-bug-fix-review-runs-a-mutation-check-2026-10-30).)
6. **Tests never hard-code the app version;** they read it from its single source and check its shape.
7. **Tests never touch the user's machine:** no real launchd/Login Items/services, no installed apps, never the
   installed Google Chrome (pinned Chrome for Testing or an explicit env var only), no `~/Documents`, `~/Desktop`,
   `~/Downloads`, no real user data, no windows on the desktop, no processes left behind. Docker: only containers you
   created, each with `--rm` and CPU and memory limits (for example `--cpus 4 --memory 12g`). Agents never restart, stop
   or reconfigure shared infrastructure (Docker or OrbStack, CI runners, the test VM outside its lock); if it is down or
   out of resources, the agent stops and reports instead of fixing it (2026-11-11; ER-14). *Why (added 2026-11-11):* an
   uncapped test container ran OrbStack's VM out of memory; the agent then restarted OrbStack, which also restarts every
   container of the user's, and Docker was down for about 15 minutes.
8. **Every lesson goes into [lessons.md](https://dev.decryptology.net/decryp7/paddock/src/branch/main/docs/lessons.md)**, every rule here; no `Co-Authored-By` or AI attribution anywhere.
9. **Releases only from a tag in CI**, never from a local machine; a local build for the user to try is fine but is never
   published. Never tag a `[skip ci]` commit.

### Long operations and performance
10. **Every operation runs on a background thread or worker pool,** never on the UI thread, the Tauri main thread or an
    IPC handler's thread; commands return at once and report through job events.
11. **Live, phase-labelled progress from the first moment,** each phase on its own scale (a growing count when the total
    is unknown, never a static 0); progress values never go backwards; Cancel works in every phase and leaves state
    consistent (build new data, then swap).
12. **The UI never times out a running job;** it shows "Still working".
13. **Slow means two questions: more parallelism, or a cheaper operation** (merged 2026-11-12, user, with former rules 14,
    15 and 51; ER-20). Measure both, in every design and every performance review. (a) Parallel where it helps, bounded by
    the kind of work (CPU: cores−1 or the Performance setting; SSD I/O 4-8; HDD 1-2; network a few per host; memory-heavy
    by RAM), at a lowered priority that never starves (rule 52), with deterministic results. Multi-stage work is a pipeline:
    producers → bounded queue → workers, stage-specific thread counts, back-pressure, cancel across stages, per-stage
    progress; determinism by commit-time ordering. IDs: number only new items in sorted canonical relative path order;
    existing IDs never change; moved or renamed files are matched by path, then fingerprint. Hash while writing; never
    re-read a just-downloaded file. Large downloads are segmented (Range) only where measured slow and the host supports
    it, with per-host caps and 429 back-off. (b) Cheaper: less work per item, no repeated work, a cheaper algorithm or
    format, caching, batching, skipping. Thread counts and similar tunables are Settings values with an "Auto" default,
    validated in the engine, not constants. Every design or ADR with a long or bulk operation has a "Parallelism and cost"
    section. Results are recorded under rule 48.
    *Why:* Debrief's import was bound by 2 Tantivy writer threads and a single-threaded catalogue, and regex was 38% of parse
    time; these were found by accident, and should be found by design.
14. **Retired 2026-11-12 (user): merged into rule 13** (pipelines and ID ordering).
15. **Retired 2026-11-12 (user): merged into rule 13** (hash while writing; segmented downloads only where measured slow).
16. **Performance has budgets** (startup, import throughput, time to first event, query latency, scroll smoothness,
    memory) checked in the nightly benchmark workflow.

### User interface
17. **Every dialog is a real OS window** (owned by and centred on the main window, remembered size/position, Escape/Enter);
    modality is explicit per dialog (modal for confirmations, credentials, unsaved changes, pickers that feed the next step;
    modeless for tools kept open beside the work). A dialog window is sized by its kind's size mode (ADR 0043): decisions, forms and
    viewers follow their content (a viewer only grows), a tool keeps the size it was given; a user resize makes it manual for that
    opening. Menus, dropdowns, autocomplete and tooltips stay in-app.
18. **Panes lay out by container queries;** paths and file names truncate in the middle with the full value in a tooltip;
    no horizontal scroll at any width ≥ 320 px.
19. **A control that repairs X never depends on X;** every "X is not running" message has a working Repair button; never
    a message that points at the page it is shown on; an installable thing is never listed as "planned".
20. **Search finds identifiers however they are written** (parts, joins, separators, case, typos) and the AI resolves the
    user's words to concrete values before querying.
21. **Tunables are Settings, knowledge lists are TOML packs** (built-in + user `packs/`, parsed once with limits, never
    executing code, reloaded without a restart).
22. **Every user-facing image (icons, store and README screenshots, product UI passes) gets an independent review;** an
    ADR or plan diagram is checked by its author (narrowed 2026-11-12, user; ER-23). Every Paddock image or status change
    also refreshes the profile repo.

### Platform and release
23. **macOS bundles are ad-hoc sealed** (`signingIdentity: "-"`) and CI verifies them with `codesign --verify --deep
    --strict` on the .app and the DMG; release verification runs it on the published DMG. While ad-hoc, the hardened
    runtime is off (ad-hoc code under it cannot load vendor-signed libraries: "different Team IDs"); every bundled
    Mach-O carries the same TeamIdentifier, and CI starts every sidecar from the built app (`--self-test`). The hardened
    runtime comes back with a real signing identity.
24. **Services survive updates:** re-register once per version change (unregister → `launchctl bootout` → wait →
    register) and repair a stale registration automatically.
25. **Before tagging:** `cargo deny check advisories`, one CI build with bundles, the icon check, strip whitespace from
    signing secrets, and the release notes' First launch section (until the apps are signed).
26. **Process memory is measured as real memory per OS:** Linux PSS (`smaps_rollup`, skipping thread entries), macOS physical footprint (`ri_phys_footprint`), Windows private working set; never RSS. Caps, budgets and displays use this one measure from `paddock-proc`.

### Angular and SignalStore (approved 2026-10-23; reasons: [research](https://dev.decryptology.net/decryp7/paddock/src/branch/main/docs/research/2026-10-angular-and-signalstore.md))
27. Zoneless only. No `zone.js` in any app, no `provideZoneChangeDetection`. Every component declares `changeDetection: ChangeDetectionStrategy.OnPush` explicitly (v22 defaults to it, but explicit survives a lint or a future default change); `Eager` requires a comment.
28. `effect()` is only for side effects on things outside the signal graph (DOM, IPC, storage, logging, charts). State derived from other state is `computed`, `linkedSignal` or `resource`. An effect that calls `.set()`/`patchState()` needs a one-line justification comment, and `untracked()` is allowed only around the side effect, not to hide a dependency.
29. Reads of signals in an async function or effect happen before the first `await`.
30. Request state is a status union (`idle | loading | loaded | error`) from one shared `withRequestStatus` feature, never separate `loading`/`error` flags.
31. A store is changed only through its own methods; `protectedState` stays on; no `patchState` outside the store file; store files are named `*.store.ts`.
32. Provide a store at the narrowest scope (route/component/dialog window); `providedIn: 'root'` only for app-wide singletons. A store injects at most two other stores; more means a missing facade or feature.
33. New code uses `inject()`, signal `input/model/output/viewChild`, `@if/@for/@switch` with `track`, standalone components, and `[class.x]` over `ngClass`.
34. Each OS window is its own Angular bootstrap with its own small provider set. The engine (Rust) is the single source of truth; windows share state by commands plus events and never by importing another window's store state. Dialog windows load only the providers they need.
35. Heavy, rarely used panels are lazy (`@defer` or `loadComponent`); every release records the initial chunk size and the budget is tightened to measured size plus 10%.
36. Prefer `resource`/`rxResource` for new read-only loading with cancellation; keep `withMethods` async for commands. Do not adopt `@Service`, `injectAsync` or the NgRx events plugin until documented stable.
37. New forms use Signal Forms; existing reactive forms stay until touched.
38. Tests: Vitest through the Angular unit-test builder, `await fixture.whenStable()`, store tests via `TestBed`; use CDK harnesses for Material/CDK widgets; each lifecycle effect gets a test that triggers the failure.
39. A shared store feature or component used by two apps lives in `paddock-ui`, not copied.

### Design (user, 2026-10-24)
40. **Recurring bugs mean a missing design rule.** When the same kind of bug shows up again (in one area, across dialogs,
    panes or apps), stop fixing instances: name the pattern, propose a redesign as an ADR that makes the class impossible
    or checked (a declared contract, one shared mechanism, a lint or test over every instance), and fix the instances
    through it. Until v1, check every batch of bug reports for such patterns.
    The coordinator proposes the redesign in the same reply that relays the report (a question to the user, or a start under
    an earlier approval), never ending a turn with an unproposed class (ER-21; hook `.claude/hooks/recurring-guard.sh` planned).
    Hooks that judge an agent's text (the recurring guard, `.claude/hooks/recurring-guard.sh`, advisory and being added) only warn; hooks that guard
    state (the cd guard, safe-remove) still block (user, 2026-11-12).

### Throughput (user, 2026-10-24)
41. **Scale parallel work to the machine.** The coordinator watches load and free disk continuously and keeps the
    machine busy but not overloaded: start more agents when load stays low, hold or stop starting them when it stays
    high (Mac mini, 14 cores: aim for a 1-minute load of about 6–16, at most ~5 heavy jobs, `CARGO_BUILD_JOBS=2` each),
    use the shared build cache (`sccache`) and copy-on-write target folders, and never let tests crowd out a release.

### Sharing (user, 2026-10-24)
42. **Share first.** Before building anything in an app, check Paddock's shared crates and packages (crates/README.md,
    ui/README.md, docs/plans/) for it. If it exists, use it; if it is missing, generic, or another app has or will need the
    same thing (rule 39), build it in Paddock and consume the published version instead of writing an app copy. Every
    work brief includes this check; every app move and every few releases a survey looks for copies that drifted apart
    and turns them into shared work packages. App-specific domain logic stays in the app.

**Testing shared code (user, 2026-10-24)**
43. **Shared behaviour is tested once, in Paddock.** A shared crate or package carries the full tests of its behaviour
    (unit, robustness, fuzz, real-OS where it matters). An app tests only its wiring, through the shared contract tests
    (`paddock-e2e`, `paddock-testkit`, conformance), its own domain logic, and one real-app smoke per release. When an app
    adopts a shared package it deletes its own copy and the tests that duplicated it, and records test count and time
    before and after.

**Settings and images (user, 2026-10-27; ADRs 0016-0019, rule 40)**
44. **Every registered settings section has at least one setting,** checked by a test or lint over every section an
    app registers (engine `SettingsSection`s in `crates/paddock-settings/src/schema.rs` and each app's sections, and
    UI-only pages in the settings navigation). A status page with only buttons belongs elsewhere (Health, a Repair action,
    or the feature's own view), or gains its setting. A plugin fallback section that explains why its plugin is missing
    (Debrief's `unavailable()`) is exempt. Today Debrief's Video page (`@video`) has no setting and must move or gain one.
45. **Image viewers open in fit mode by default** (the whole image visible, never upscaled past 100 %), with 100 % and
    zoom as explicit choices; the viewer fits before its first paint rather than after measuring. A shared viewer in
    `paddock-ui` (rule 39) owns the default; `notes/image/note-image-view.ts` in Debrief starts at scale 1 and must adopt it.

**Large jobs (user, 2026-10-28; Debrief research note 2026-10-28)**
46. **No LLM for large jobs.** Work over many items (imports, catalogues, name tables, indexing, classification, tagging
    or summaries per item) uses deterministic rules, shipped TOML knowledge packs (rule 21) and static embeddings; an LLM
    is used only for bounded requests the user starts (a chat turn, a chosen range, an explicit "Suggest with AI" over a
    small selection), never automatically and never once per item. An exception (for example describing all frames of a
    video) needs an explicit confirmation that states the item count and the expected time. *Why:* a local model per item
    turns a cheap background task into hours of GPU and gigabytes of memory on the user's machine (Debrief 1.1 loaded an
    8 GB model to name 2,000 log names after every import), its answers change with the model and cannot be explained, and
    rules, packs and the logs' own metadata were measured as good or better for the same job.

### Components (user, 2026-10-28)
47. **Material first.** Use Angular Material components wherever one fits (tables, menus, tooltips, tabs, selects,
    toggles, chips, steppers, trees, progress, snack bars…), and Angular CDK for behaviour Material doesn't cover;
    hand-roll only what neither provides, with the reason in the component's doc comment. Material is themed through the
    `--pd-*` tokens (one shared theme), CSP-safe. *Why:* Material is widely used and tested; hand-rolled components cost
    bugs it already solves.

### Benchmarks (user, 2026-10-29)
48. **Every quoted benchmark run is recorded.** Any benchmark or performance number quoted anywhere is first recorded in
    the owning app's `docs/performance.md` (shared code: `docs/benchmarks.md`) as a results block in the template of
    [benchmarks.md](https://dev.decryptology.net/decryp7/paddock/src/branch/main/docs/benchmarks.md), newest first. "Anywhere" means an ADR, research note, plan, CHANGELOG, commit message,
    pull request or lesson. The block gives date, commit, machine, load, build, synthetic corpus and generator, the exact
    command with env vars, the number and kind of runs, the numbers with the budget and verdict, and the memory measure
    (physical footprint, rule 26). The quoting document links to the block. A number without a block is not evidence for a
    decision or a budget. *Why:* results were scattered across ADRs, branches and expiring job logs, often without
    machine, load or build, so they could not be compared or reproduced, and stale numbers kept being quoted.

### Test isolation (user, 2026-10-29)
49. **Specs never wait a fixed number of event-loop turns and never leave a patch behind.** Specs of one bundle share a
    page (the Angular builder runs Vitest with `isolate: false`), so: (a) wait on the condition, never on a count of
    turns or timers: once `fix/debrief-import-range-flake` lands, Debrief's `openDialog()` (`testing/helpers.ts`) waits
    with `vi.waitFor` for a lazily loaded dialog and fails when none opens; (b) an optional chain never performs a
    test's action (no `stack().at(-1)?.ref.close()`): a missing thing must fail the test; (c) a patch on a prototype is
    a `vi.spyOn`, put back by `restoreMocks` and by a global `afterEach(vi.restoreAllMocks)`, never an assignment,
    `Object.defineProperty` or `Object.assign` on it. Once that branch lands, Debrief's ESLint (`no-restricted-syntax`
    for specs) enforces (b) and (c); the other apps follow. *Why:* a fixed wait and a leaked patch each made a spec
    fail now and then, only in the full run
    ([lessons.md](https://dev.decryptology.net/decryp7/paddock/src/branch/main/docs/lessons.md#test-isolation-debrief-2026-10-29)).

### CI economy (user, 2026-10-29)
50. **CI runs only what a change needs, once.**
    (a) At most one CI run per branch head. Before dispatching, cancel any queued run on the same branch with an older head.
    (b) Do not dispatch while review fixes are pending. Dispatch once after the last fix, and do not re-dispatch just to
    refresh against main; merge main right before landing.
    (c) A branch already merged into another branch gets no CI of its own.
    (d) Docs-only changes land after the local conformance checks, without waiting for CI.
    (e) Branch runs use the affected plan (rule 3); the full gates run once on main or at release.
    (f) is reserved for the push check (ADR 0049 WP-1, D2).
    (g) Before landing a branch (a merge into main), the integrator lints its commits locally:
    `node conformance/bin/paddock-conformance.mjs commit-lint --range origin/main..<branch>`. A push to main lints only what main
    itself gets (`--first-parent`), so this is where a branch's own subjects and trailers are checked; C23 is the backstop.
    (h) Local checks and CI overlap; a local failure cancels the run (ER-9). Before dispatch, run the fast static gates locally,
    in parallel and bounded by the machine's load: `cargo fmt --check`, clippy on the host and, for Windows, `cargo xwin clippy
    --target x86_64-pc-windows-msvc --workspace --all-targets -- -D warnings` (an import or helper used only by a unix-gated
    test is dead code there; ADR 0021, lessons.md "Windows-only assumptions in tests"), lint and type-check of the touched
    files, conformance, commit-lint (rule 50g), lockfiles with `--locked`. When `crates/` or `.cargo/` changed, also CI's
    workspace Clippy on Linux, `bash tools/linux-clippy-touched.sh` (the CI image, `--cpus 4 --memory 12g`; without Docker it
    exits 3, "NOT MEASURED", and an agent never starts Docker, ER-14). Then dispatch with `./dev`
    and, at the same time, run the slower local checks in parallel: the affected tests on the dev OS, the same tests in the
    `train3-linux-dev:1` container (CI limits, `ulimit -n 1024`) and timing-sensitive tests under load. Cap local builds
    (`CARGO_BUILD_JOBS=2`) while CI runners share the machine. If a local check fails, cancel that run at once through the Gitea
    API (`POST /api/v1/repos/{owner}/{repo}/actions/runs/{id}/cancel`), fix every failure and push once; this cancel needs no
    further approval. (a) and (b) still hold.
    (i) After a CI run fails, each fix round reruns only what failed in the last run: the failed jobs, and inside them only
    the failed tests (nextest or vitest filter) where the job's runner supports it. Jobs and tests that passed aren't rerun
    during fix rounds. Dispatch `ci.yml` (workflow_dispatch) with the input `base` = the commit CI last tested, not main; use
    "rerun failed jobs" on the same commit (`POST /api/v1/repos/{owner}/{repo}/actions/runs/{run}/rerun-failed-jobs`) only to
    retry a suspected flake, never to turn red green (ER-12). When every round is green, run the full affected plan against
    main once on the final tree (ER-8); that run catches anything a fix broke elsewhere. Then land. Nothing lands without that
    full green run. The planner is getting a mode for this (`./dev plan --since-run <id>` lists the failed jobs and tests of
    a run, a train-3 item); until it exists, the coordinator builds that list by hand.
    *Why:* CI runners are the scarcest resource, and every wasted run delays a release.

### Performance analysis (user, 2026-10-29)
51. **Retired 2026-11-12 (user): merged into rule 13** (more parallelism, or a cheaper operation).

### Thread priority (user, 2026-10-30)
52. **Work a person waits for never runs at an OS's starvable background level.** Starvable levels: on macOS QoS
    `BACKGROUND`, process background (`PRIO_DARWIN_BG`, `taskpolicy -b`, `-c background` or `-c maintenance`) and
    throttled I/O (`IOPOL_THROTTLE`, `taskpolicy -d throttle`); on Windows `THREAD_MODE_BACKGROUND_BEGIN`,
    `PROCESS_MODE_BACKGROUND_BEGIN` (both also drop I/O and memory priority to the lowest), `IDLE_PRIORITY_CLASS`,
    `THREAD_PRIORITY_IDLE` and EcoQoS (`ProcessPowerThrottling` / `ThreadPowerThrottling` execution speed); on Linux
    `SCHED_IDLE` (`chrt --idle`) and the idle I/O class (`IOPRIO_CLASS_IDLE`, `ionice -c3`). The same levels set for a
    whole service count too: launchd `ProcessType` `Background` (and `Adaptive` when the service has no XPC
    `MachServices` to raise it), `LowPriorityIO` or `LowPriorityBackgroundIO` `true`, systemd `CPUSchedulingPolicy=idle`
    and `IOSchedulingClass=idle`. Every other thread has a role. **Live** work is what a person waits on within a
    deadline or for a screen to fill: requests, probes with a deadline, a sampler behind a live view, a page refresh.
    **Bulk** work is long work that shows progress and can be cancelled: walks, scans, compaction, exports. Bulk work
    runs at the gentle level: `QOS_CLASS_UTILITY` on macOS, nice 10 (with best-effort I/O) on Linux,
    `THREAD_PRIORITY_BELOW_NORMAL` on Windows (Debrief's `debrief_core::threads::lower_priority`; family ADR 0020's
    `Background` class); a busy machine slows it, and on Windows it accepts stalls of seconds. A thread holding a lock
    that live work waits on is live, or must not hold the lock across slow work. Live work may use the
    gentle level on macOS and Linux, whose schedulers share the CPU by weight, but stays at `THREAD_PRIORITY_NORMAL` on
    Windows: Windows always runs a ready thread of a higher priority before a lower one
    ([Scheduling Priorities](https://learn.microsoft.com/en-us/windows/win32/procthread/scheduling-priorities)), its
    dynamic boosts are for foreground windows, input and finished waits
    ([Priority Boosts](https://learn.microsoft.com/en-us/windows/win32/procthread/priority-boosts)), and its
    anti-starvation boost for a ready thread that has not run comes only after seconds (Windows Internals; Microsoft
    Learn's [Priority Inversion](https://learn.microsoft.com/en-us/windows/win32/procthread/priority-inversion) page now
    describes AutoBoost for lock owners instead). A starvable level is only for work nobody waits on (ADR 0020's `Idle`
    class), and each use says why with a `conformance: background-qos — <why nobody waits on it>` comment (check C54). (`THREAD_PRIORITY_LOWEST` is not on the list, but like `BELOW_NORMAL` it waits behind busy
    normal threads, so it is for bulk work only; `THREAD_PRIORITY_IDLE` sits below all of the dynamic band.) *Why:* at
    these levels a thread gets the CPU or the disk only after all other work at higher levels, for as long as there is
    any (macOS `BACKGROUND` also keeps it to the efficiency cores), so a caller waiting on it waits as long as the
    machine is busy. Debrief's mining pool ran in QoS `BACKGROUND`, and on the loaded CI Mac `debrief report` waited
    minutes for its analyses and timed out (Debrief ADR 0052, amended 2026-10-30; lessons 2026-10-30). On the Windows CI
    runner Dyno's sampler and counters listing, lowered to `BELOW_NORMAL`, did not run for seconds while tests filled the
    cores (CI runs 7166 and 7168; lessons 2026-10-30).

### Process identity (user, 2026-10-30)
53. **Never act on a reusable OS name read earlier.** A process id, a port or a temp path read earlier may name something
    else by the time it is used. Hold the thing itself (a process handle, a job object, a bound listener, an open file)
    or verify its identity immediately before acting (for a process: its pid plus its start time,
    `paddock_proc::ProcessId`). A tree is followed by age (a child never started before its parent) or by job
    membership, never by parent pids alone. Kills go through `paddock_proc` (`kill_tree`, `end`, `ask`, `terminate`,
    `Supervised`). Clippy's `disallowed-methods` (lint/clippy.toml: `libc::kill`, `libc::killpg`, nix's `kill`/`killpg`,
    sysinfo's `Process::kill`/`kill_with`) and conformance check C53 (`taskkill`, kill commands, `Stop-Process`,
    `TerminateProcess`, `TerminateJobObject`, open ADR 0033 follow-ups) enforce it. ADR
    [0033](https://dev.decryptology.net/decryp7/paddock/src/branch/main/docs/adr/0033-processes-by-identity-never-by-a-bare-pid.md); ADR [0023](https://dev.decryptology.net/decryp7/paddock/src/branch/main/docs/adr/0023-tests-never-free-a-port-they-probe.md)
    is the same rule for ports.
    A discovery file, a file that names a pid or a port (Dyno's `agent.json`), is only a hint: act on it only after a held
    lock or a live connection proves the process is there, and remove it only if it is still the same file (same pid and
    start time), never because it was the file read earlier.
    *Why:* Windows hands a freed pid on at once. `kill_tree` killed the pids of a stale snapshot, which can end another
    test's Chrome helper; the same mistake was in four shapes in five apps. (It was suspected of causing the Windows
    `ERR_ABORTED` failures; the suspected cause was disproved by run 7158; the flake has another cause, ADR 0033 WP-4.)
    Dyno's stale `agent.json` made the app say "Agent stopped" and loop on exit 3 for a process that was gone or replaced
    ([lessons.md](https://dev.decryptology.net/decryp7/paddock/src/branch/main/docs/lessons.md#a-discovery-file-is-a-hint-never-proof-2026-10-30-dyno)).

### Release path (user, 2026-10-30)
54. **Every change that touches an app's release path builds that app's installers before it lands on main, once per
    train or batch that touches it (user, 2026-11-12), not once per branch.** The release path is the release workflow,
    the shared app-release workflow, the bundle and prepare steps, the Tauri config and the app's imports of shared
    packages.
    (a) The batch dispatches `<app>-release.yml` with gates on and the installers (bundle) built; passing tests alone is
    not enough.
    (b) Partly enforced: the ci-actions test (added on fix/dyno-bundle-ui) checks that every release workflow whose app
    imports `@decryp7/paddock-ui` builds the UI.
    (c) Amendment of 2026-11-09 (ADR 0080 D13, accepted by the user): the release workflow dispatched for this rule
    skips its gates only when a green `ci.yml` run on the same commit covered every gate it would run, compared job by
    job (the affected plan and ADR 0080 D12 can leave legs out of `ci.yml`, and the release workflow then runs those
    legs, rule 71); the installers are always built.
    *Why:* Dyno's release workflow had no `prepare: npm run build -w ui`. Tests passed, but every installer build failed
    on a clean runner; a leftover `ui/dist` had hidden it.

### Copies (user, 2026-10-31; ADR 0034, rule 40)
55. **One implementation per mechanism; copies are registered and fixed together** (no trailers since 2026-11-12, user;
    the register is a to-do list). A mechanism that exists more than once (a shared crate or package and an app's copy,
    or two apps' copies) is listed in `docs/known-copies.toml` with its canonical implementation and, for each copy, the
    app that deletes it and the tag by which it is gone.
    (a) A change that touches one member of a listed mechanism (its non-test source) makes the same change in every
    other copy and in a built canonical, each with its own failing test first (rule 11), or names the copy it skipped,
    and why, in the commit body. Hand-written plugin wiring (`kind = "wiring"`) is held to its delete-by tag only.
    (b) A shared-code freeze (rule 1) does not freeze fixes to a listed mechanism: the fix lands in the canonical
    implementation and every copy in the same change. The shared API stays frozen.
    (c) A new copy is allowed only with its register entry, a delete-by tag and the reason, in the same change. An entry
    leaves the register with its copy's files.
    (d) A copy whose delete-by tag is reached is reported as a warning; it no longer blocks a tag (user, 2026-11-11: the
    register is a to-do list). A delete-by date moves to a later release line (a higher major or minor version) only
    with the user's approval, with the reason in the commit body.
    (e) Enforced by conformance check C58 (planned, ADR 0034 WP-1; not built yet). A register-only change is docs-only
    and gets no CI run (rule 50), so the steward checks the register by hand before it lands. Once C58 exists, batches
    run `copies --range <base>..HEAD` before pushing, and the steward runs `copies --tag <tag>` before creating a tag.
    Until C58 exists, the reviewer and the steward check the register by hand.
    *Why:* launchd's job report existed four times. The shared one already skipped nested blocks; Dyno's copy did not
    and replaced its healthy agent at every login (Dyno 1.0 blocker B4). The Windows network-sandbox fix of the browser
    driver sat in the one copy no app used, and the EBADF fix had to be made in two IPC endpoints.

### Release checks (user, 2026-11-05; ER-33)
71. **Retired 2026-11-12 (user): merged into ER-33** (a release check counts only if its precondition was verified and its probe has shown it can see the thing; otherwise `not measured`).

### Design decisions (user, 2026-11-11; ER-37)
76. **Measure a design against common practice before paying for it.** Before an ADR or design is accepted, it states how
    comparable, well-known software handles the same problem (for example Visual Studio, VS Code, Chrome, Docker, Homebrew,
    git, the OS vendor's own tools) and what the platform vendor documents as the standard (Microsoft, Apple, Linux
    distributions), with sources. It then states the cost in agent-days, set against that baseline. A design that goes
    beyond common practice says why this project needs more, and the cheaper common-practice option is always offered as a
    decision. Personal, single-user tools default to common practice.
    A design, mechanism or fix costing more than 1 agent-day beyond the common-practice option needs the user's yes, with
    both costs shown; a standing "build everything" OK does not cover it. Each release plan states its budget in raw
    agent-days when its scope is agreed, and growth beyond 20% goes back to the user. Each release gets one cut review, at
    scope freeze, by one reviewer, against this rule (ER-37, amended 2026-11-12).
    *Why:* on 2026-11-11 a 23-31-day elevated-helper design for Debrief went through four review rounds before the user
    compared it with Visual Studio and VS Code and chose a half-day risk warning instead; several accepted process ADRs
    (signed approvals, decision registers, extra gates) were then cut as more than a personal tool needs.
    On 2026-11-12 a cut review found Debrief 1.2 grown to about 656 raw days and Pitcrew's 1-2 day caller check in its 8th
    review round.

### Privileged services (user, 2026-11-11; ER-38)
77. **Privileged services check the caller.** A service or helper that runs as administrator, root or SYSTEM authorises
    every request that changes something by the caller's identity, read from the OS (pipe client token, peer
    credentials), not by the pipe or socket ACL alone. A request that changes what runs elevated (a program path, a
    folder, a setting the elevated side acts on) needs an administrator caller. Read-only requests may take any local
    user the ACL lets in. Elevation the user chooses with a click (Run or Restart as administrator) needs only a clear
    risk warning, as tools such as Visual Studio and VS Code allow elevation with no further controls (they mark the
    window as Administrator). User-chosen elevation is governed by this rule, not rule 75.
    *Why:* on 2026-11-11 a survey found that Pitcrew's LocalSystem service (shipped in pitcrew-v1.0.1) starts any program
    a same-user process names over its pipe, with no prompt, because ADR 0004 authorised by the pipe ACL alone. Dyno's
    helper already checks the caller per connection. The user chose "warn and accept" for click-to-elevate and a caller
    check for no-click paths.

### Monorepo (ADR 0014 §10; effective for an app from its import into `paddock/apps/<app>`, for Paddock from the first import)
Until an app is imported, rules 2, 3, 8, 9, 39 and 42 above apply to it as written. From its import, these replace them:
- **Rule 2:** every agent works in its own git worktree of `paddock` (sparse to its app where possible:
  `git sparse-checkout set --cone crates ui scripts docs apps/<app>`), with its own `CARGO_TARGET_DIR` per app and its own
  e2e port; the main checkout stays on `main` for the coordinator.
- **Rule 3:** the affected tests are what `scripts/affected.mjs` selects, plus the touched specs; otherwise unchanged
  (Playwright in Chromium only in a branch; the full suite, Chromium and WebKit, once on the combined result before a
  local build or release, plus CI).
- **Rule 8:** rules live once, in this file and the root `AGENTS.md`; no per-app copies. Every lesson still goes into
  [lessons.md](https://dev.decryptology.net/decryp7/paddock/src/branch/main/docs/lessons.md); no `Co-Authored-By` or AI attribution anywhere.
- **Rule 9:** releases only from a `<app>-vX.Y.Z` tag in CI, never from a local machine; a local build for the user to
  try is fine but is never published. Never tag a `[skip ci]` commit.
- **Rules 39 and 42:** shared code is moved to `crates/` or `ui/` and adopted by the app **in the same change**; there is
  no publish-and-consume step. "Consume the published version" becomes "use the in-tree version".
- Rule 1 is about attention, load, CI capacity and shared-code conflicts, not repos (amended 2026-10-28: one release at a
  time; other apps only within those limits).
<!-- /paddock:block rules -->

<!-- paddock:block design-principles rev=7910d082 -->
## Design principles

Apply these deliberately. They are not decoration:

- **Single responsibility.** One reason to change per module, type and component. Split a file when it starts doing
  several jobs; keep request/response DTOs apart from the logic that produces them.
- **Open/closed.** Extend by adding, not editing: a new kind of thing is a new implementation of an existing trait or a
  new configuration entry, not a special case in shared code.
- **Liskov.** Every implementation of a trait or interface honours its documented contract: same error semantics, no
  surprising panics.
- **Interface segregation.** Keep traits and TypeScript interfaces small and purpose-built.
- **Dependency inversion / code against abstractions.** Tauri commands depend on traits (ports), never on a concrete
  store, pipe or OS API. Angular components never call Tauri `invoke` directly: they go through the backend injected by
  DI token (the real Tauri backend, or the mock for browser development and tests), and state lives in SignalStores.
- **Pure core, thin edges.** Parsing, rules, formatting, geometry and state machines are pure functions with time and
  I/O passed in; processes, files, network and the OS stay thin.
- **Ports and adapters.** The core never depends on an adapter (Tauri, a database, an OS API).
- **Parse, don't validate.** Convert untrusted input (file bytes, IPC JSON, config files, network replies) into typed
  values once, at the edge, and work with the types afterwards.
- **Make illegal states unrepresentable.** Prefer enums and newtypes over strings, flags and `Option` combinations that
  can disagree.
- **Command-query separation.** Queries have no side effects; only explicit commands mutate.
- Also: separation of concerns, high cohesion and low coupling, KISS, YAGNI, DRY with the rule of three before
  abstracting, composition over inheritance, Law of Demeter, least astonishment (sensible defaults, per-item overrides),
  fail fast with a message that tells the user what to fix, and leave touched code better than you found it.
<!-- /paddock:block design-principles -->

## Security and robustness (untrusted pages and feeds)

- Never trust the webview; validate every IPC argument in Rust. Open only paths the user chose; canonicalize them.
- Everything fetched is untrusted: HTTPS preferred, redirects capped, bodies capped (`MAX_BODY`), timeouts on every
  request, and the per-site minimum interval and `ETag`/`Last-Modified` always honoured. No requests to the loopback or
  link-local ranges unless the watch is explicitly local. Never render fetched text with `[innerHTML]`; fetched HTML is
  parsed, never executed, except inside the renderer.
- **Renderer rules (ADR 0002).** Pages run in a separate supervised Chromium process with a throwaway per-watch
  profile, hard caps on time and memory, no extensions, no stealth tricks. Readiness = load finished + element present
  + DOM quiet (not network idle). A challenge (login, captcha) is handed to the user as "Needs you", never solved.
  Tokens and cookies never leave the credential store/profile and are never logged.
- User regexes only through `user_regex::compile` (length and compiled-size caps). Parsers never panic and enforce
  limits. No `unwrap()`/`expect()` on user data or I/O outside tests. Library errors are typed.
- Tauri capabilities stay least-privilege; CSP free of `unsafe-eval` and remote origins. External tools and child
  processes run with timeouts and no console window on Windows. Logs contain no fetched content or secrets.
- Persistent state (config, SQLite) is versioned, written atomically and tolerates missing/corrupt files by falling
  back to defaults. Every robustness behaviour has a test that triggers the failure.

## Languages

All shipped code is Rust plus the Angular UI. `unsafe` is denied workspace-wide. No Python/C#/C++ in the product.

<!-- paddock:block where-logic-lives rev=aceff590 -->
## Where logic lives

- **The Rust core is the single source of truth** for data and behaviour: parsing, rules, state machines, configuration,
  analysis and exports. Results are computed in Rust and returned through ports and Tauri commands.
- **Angular is the presentation layer**: layout, interaction state (selection, zoom, open panels, unsaved form edits),
  rendering and formatting of what the engine returns. It must not re-implement domain rules. If the UI needs a
  computation (a preview, a validation, a count), add an engine operation and a command for it instead of writing it in
  TypeScript. Small purely visual transforms are fine.
- **The mock backend** exists for browser development and UI tests only. It mimics the contract with plausible data; it
  is never the reference for behaviour, and a feature is not "done" until it works against the real engine.
- Validation that matters is done in Rust; the UI may validate early for convenience, but the engine re-validates.
<!-- /paddock:block where-logic-lives -->

<!-- paddock:block working-in-parallel rev=458b77ad -->
## Working in parallel (agents)

- Each task owns an explicit set of files; don't edit outside it. Shared integration points (`lib.rs` files, the
  workspace `Cargo.toml`, the wire types and DTOs) are changed by the integrating agent only, or by adding a single
  declared line.
- Don't run `cargo fmt --all` / `cargo clippy --fix` across the workspace while others are editing; format your own
  files (`rustfmt <files>`). Use a private `CARGO_TARGET_DIR`.
- Commit only complete, owned work. Never commit a workspace `Cargo.toml` listing crates whose sources aren't
  committed. After committing, verify the committed tree in isolation (`git archive HEAD`, then build and test) before
  pushing larger changes.
- Windows-only and macOS-only code is checked and tested on its own CI runner; code that needs no platform SDK must at
  least type-check for the other targets (`cargo check --target x86_64-pc-windows-msvc`).
<!-- /paddock:block working-in-parallel -->

## Contracts

The UI-backend contract is `app/src/app/core/api.types.ts` (camelCase JSON), mirrored by `app/src-tauri/src/dto.rs`;
change both in one commit. Config and state are versioned (`SCHEMA`), with migration and a test.

## Testing and quality gates

Every change ships with tests; every bug fix starts with a failing regression test. Targets: `spotter-core` >= 99%
lines, Angular >= 80% overall and near 100% for stores/services/utilities. Before committing:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny --workspace check
node --test scripts/pin-chromium.test.mjs scripts/install-test-browser.test.mjs scripts/style-guard.test.mjs scripts/ci/check-icons.test.mjs
(cd ../.. && node --test conformance/test/*.test.mjs)   # the shared scripts: commit lint, latest.json, publish, check-index
(cd app && npm audit --omit=dev --audit-level=high && npx ng lint && npx ng build && npx ng test --watch=false)
```

## Dependencies and releases

Permissive licences only (`deny.toml`). Commit `Cargo.lock` and `package-lock.json`. Conventional Commits (no AI
attribution lines); releases are `spotter-v*` tags. Significant decisions are ADRs in `docs/adr/`.

<!-- paddock:block style rev=e5db9004 -->
## Style

- Match the surrounding code: naming, comment density (doc comments on public items, comments only where intent isn't
  obvious), idioms. Rust uses `rustfmt.toml` (max_width 140). Never run `prettier --write` on TypeScript.
- UI: standalone components, signals, OnPush, CSS custom properties for theme tokens (dark and light), accessible markup
  (real buttons, labels, aria-labels on icon buttons), sentence-case copy, verb plus object on buttons.
- UI building blocks: use **Angular CDK** for behaviour and accessibility (menus, overlays, dialogs, listbox, scrolling)
  instead of hand-rolling them, and **Angular Material first** (rule 47: use a Material component wherever one fits), themed through the
  `--pd-*` tokens. Icons are inline SVG registered with `MatIconRegistry`; no icon fonts and no icon URLs fetched at run time (the
  CSP forbids it).
- Status colours always come with words; validate colours for contrast and colour-vision deficiency.
<!-- /paddock:block style -->

<!-- paddock:block working-practices rev=23b72d08 -->
## Working practices

- Research official sources before choosing libraries or relying on another program's behaviour, and record what you
  found and when in the ADR or doc comment. Prefer well-maintained, permissively licensed dependencies.
- Navigate code with Serena's symbol tools (find symbol, references, overview) when they are available instead of
  reading whole files; it saves context and cost.
- Delegate mechanical or well-specified work (refactors with a precise spec, test scaffolding, docs) to cheaper models.
  Keep architecture and review on the main model.
- Share first (rule 42): check Paddock's crates and packages before writing anything an app copy would duplicate.
- Record every benchmark you quote (rule 48): first as a results block in the app's `docs/performance.md` (shared code:
  `docs/benchmarks.md`), then link it from the ADR, plan, CHANGELOG, commit or PR that quotes the number.
- Specs never wait a fixed number of event-loop turns and never leave a patch behind (rule 49): wait on the condition
  (Debrief's `openDialog()`, once `fix/debrief-import-range-flake` lands), never let an optional chain perform a test's
  action, and patch a prototype only with `vi.spyOn` (restored after each test).
- Spend CI only on what a change needs (rule 50): one run per branch head, none while review fixes are pending or for a
  branch already merged elsewhere, none for docs-only changes; branches run the affected plan, full gates once on main.
- A change that touches an app's release path builds that app's installers before it lands on main (rule 54): the batch
  dispatches `<app>-release.yml` with gates on and the bundle built, once per train or batch that touches the release path;
  passing tests alone is not enough.
- One implementation per mechanism (rule 55): a mechanism listed in `docs/known-copies.toml` is fixed in its canonical
  implementation and every copy in the same change, each with a failing test first, or names the copy it skipped, and why, in the commit body; a freeze does not stop such a fix; a new copy needs its entry and a delete-by tag, an entry leaves
  with its files, and a date moves to a later release line only with the user's approval; a reached delete-by tag is a warning, not a tag block (user, 2026-11-11); register check C58 (planned, ADR 0034 WP-1; not built yet, the steward checks the register by hand).
- Slow means two questions (rule 13, merged with 14, 15 and 51): can more threads or workers run it (bounded by the kind of work), and can it be made
  cheaper (less work per item, no repeats, a cheaper algorithm or format, caching, batching, skipping)? Measure both;
  thread counts are Settings with an "Auto" default; a long or bulk operation's design has a "Parallelism and cost" section.
- Never act on a reusable OS name read earlier (rule 53): hold it (handle, job, listener) or verify its identity (pid plus
  start time, `paddock_proc::ProcessId`) immediately before acting; kills go through `paddock_proc` (Clippy
  `disallowed-methods` and C53 check it, ADR 0033). A discovery file that names a pid or port (Dyno's `agent.json`) is
  only a hint: act on it after a held lock or a live connection proves the process is there, and remove it only if it is
  still the same file (same pid and start time).
- Work a person waits for never runs at an OS's starvable background level (rule 52: macOS QoS `BACKGROUND`,
  `PRIO_DARWIN_BG`, `taskpolicy -b`/`-c background`/`-c maintenance`/`-d throttle` and `IOPOL_THROTTLE`; Windows
  background modes, idle priorities and EcoQoS; Linux `SCHED_IDLE` and idle I/O; for a service, launchd `ProcessType`
  `Background` (or `Adaptive` without XPC) and `LowPriorityIO`, systemd `CPUSchedulingPolicy=idle` and
  `IOSchedulingClass=idle`): bulk work (long, with progress and Cancel: walks, scans, compaction, exports) uses
  `QOS_CLASS_UTILITY`, nice 10 or `THREAD_PRIORITY_BELOW_NORMAL`; live work (a person waits on it within a deadline or
  for a screen to fill: requests, probes, a sampler, a page refresh) may use the first two but stays at the normal level
  on Windows; a thread holding a lock that live work waits on is live, or must not hold the lock across slow work; a
  starvable level only for work nobody waits on, with a `conformance: background-qos — <why>` comment (C54).
<!-- /paddock:block working-practices -->
