# Paddock rules

Binding rules for every Paddock app and for everyone (people and agents) working on them. Each rule comes from a user
decision or a lesson; the reasons are in [lessons.md](lessons.md). Each app's `AGENTS.md` carries these rules; when a rule
changes here, every app's copy is updated in the same change.

## Way of working
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
6. **Tests never hard-code the app version;** they read it from its single source and check its shape.
7. **Tests never touch the user's machine:** no real launchd/Login Items/services, no installed apps, never the
   installed Google Chrome (pinned Chrome for Testing or an explicit env var only), no `~/Documents`, `~/Desktop`,
   `~/Downloads`, no real user data, no windows on the desktop, no processes left behind. Docker: only containers you
   created.
8. **Every lesson goes into [lessons.md](lessons.md)**, every rule here; no `Co-Authored-By` or AI attribution anywhere.
9. **Releases only from a tag in CI**, never from a local machine; a local build for the user to try is fine but is never
   published. Never tag a `[skip ci]` commit.

## Long operations and performance
10. **Every operation runs on a background thread or worker pool,** never on the UI thread, the Tauri main thread or an
    IPC handler's thread; commands return at once and report through job events.
11. **Live, phase-labelled progress from the first moment,** each phase on its own scale (a growing count when the total
    is unknown, never a static 0); progress values never go backwards; Cancel works in every phase and leaves state
    consistent (build new data, then swap).
12. **The UI never times out a running job;** it shows "Still working".
13. **Parallel where it helps, bounded by the kind of work** (CPU: cores−1 or the Performance setting; SSD I/O 4–8; HDD
    1–2; network a few per host; memory-heavy by RAM), background priority, deterministic results.
14. **Multi-stage work is a pipeline:** producers → bounded queue → workers, stage-specific thread counts, back-pressure,
    cancel across stages, per-stage progress; determinism by commit-time ordering. IDs: number only new items in sorted
    canonical relative path order; existing IDs never change; moved/renamed files are matched by path, then fingerprint.
15. **Hash while writing;** never re-read a just-downloaded file. Large downloads use segmented (Range) downloads where
    the host supports them, with per-host caps and 429 back-off.
16. **Performance has budgets** (startup, import throughput, time to first event, query latency, scroll smoothness,
    memory) checked in the nightly benchmark workflow.

## User interface
17. **Every dialog is a real OS window** (owned by and centred on the main window, remembered size/position, Escape/Enter);
    modality is explicit per dialog (modal for confirmations, credentials, unsaved changes, pickers that feed the next step;
    modeless for tools kept open beside the work). Menus, dropdowns, autocomplete and tooltips stay in-app.
18. **Panes lay out by container queries;** paths and file names truncate in the middle with the full value in a tooltip;
    no horizontal scroll at any width ≥ 320 px.
19. **A control that repairs X never depends on X;** every "X is not running" message has a working Repair button; never
    a message that points at the page it is shown on; an installable thing is never listed as "planned".
20. **Search finds identifiers however they are written** (parts, joins, separators, case, typos) and the AI resolves the
    user's words to concrete values before querying.
21. **Tunables are Settings, knowledge lists are TOML packs** (built-in + user `packs/`, parsed once with limits, never
    executing code, reloaded without a restart).
22. **Every drawing, diagram and screenshot gets an independent review;** every Paddock image or status change also
    refreshes the profile repo.

## Platform and release
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

## Angular and SignalStore (approved 2026-10-23; reasons: [research](research/2026-10-angular-and-signalstore.md))
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

## Design (user, 2026-10-24)
40. **Recurring bugs mean a missing design rule.** When the same kind of bug shows up again (in one area, across dialogs,
    panes or apps), stop fixing instances: name the pattern, propose a redesign as an ADR that makes the class impossible
    or checked (a declared contract, one shared mechanism, a lint or test over every instance), and fix the instances
    through it. Until v1, check every batch of bug reports for such patterns.

## Throughput (user, 2026-10-24)
41. **Scale parallel work to the machine.** The coordinator watches load and free disk continuously and keeps the
    machine busy but not overloaded: start more agents when load stays low, hold or stop starting them when it stays
    high (Mac mini, 14 cores: aim for a 1-minute load of about 6–16, at most ~5 heavy jobs, `CARGO_BUILD_JOBS=2` each),
    use the shared build cache (`sccache`) and copy-on-write target folders, and never let tests crowd out a release.

## Sharing (user, 2026-10-24)
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

## Components (user, 2026-10-28)
47. **Material first.** Use Angular Material components wherever one fits (tables, menus, tooltips, tabs, selects,
    toggles, chips, steppers, trees, progress, snack bars…), and Angular CDK for behaviour Material doesn't cover;
    hand-roll only what neither provides, with the reason in the component's doc comment. Material is themed through the
    `--pd-*` tokens (one shared theme), CSP-safe. *Why:* Material is widely used and tested; hand-rolled components cost
    bugs it already solves.

## Benchmarks (user, 2026-10-29)
48. **Every quoted benchmark run is recorded.** Any benchmark or performance number quoted anywhere is first recorded in
    the owning app's `docs/performance.md` (shared code: `docs/benchmarks.md`) as a results block in the template of
    [benchmarks.md](benchmarks.md), newest first. "Anywhere" means an ADR, research note, plan, CHANGELOG, commit message,
    pull request or lesson. The block gives date, commit, machine, load, build, synthetic corpus and generator, the exact
    command with env vars, the number and kind of runs, the numbers with the budget and verdict, and the memory measure
    (physical footprint, rule 26). The quoting document links to the block. A number without a block is not evidence for a
    decision or a budget. *Why:* results were scattered across ADRs, branches and expiring job logs, often without
    machine, load or build, so they could not be compared or reproduced, and stale numbers kept being quoted.

## CI economy (user, 2026-10-29)
50. **CI runs only what a change needs, once.**
    (a) At most one CI run per branch head. Before dispatching, cancel any queued run on the same branch with an older head.
    (b) Do not dispatch while review fixes are pending. Dispatch once after the last fix, and do not re-dispatch just to
    refresh against main; merge main right before landing.
    (c) A branch already merged into another branch gets no CI of its own.
    (d) Docs-only changes land after the local conformance checks, without waiting for CI.
    (e) Branch runs use the affected plan (rule 3); the full gates run once on main or at release.
    *Why:* CI runners are the scarcest resource, and every wasted run delays a release.

## Performance analysis (user, 2026-10-29)
51. **Slow means: more parallelism, or a cheaper operation.** When any operation is slow (import, indexing, scans, queries,
    CI steps, tests), the analysis answers two questions with measurements:
    (a) Can more threads or workers be assigned? Bounded by the kind of work (ER-20): CPU-bound work gets cores minus one,
    I/O a few, memory-heavy work is limited by RAM; it runs at background priority with deterministic results.
    (b) Can the operation be made cheaper? For example less work per item, no repeated work, a cheaper algorithm or format,
    caching, batching, skipping.
    Thread counts and similar tunables are Settings values with an "Auto" default, validated in the engine, not
    constants. Every design or ADR with a long or bulk operation has a "Parallelism and cost" section. Every
    performance review checks both questions. Results are recorded under rule 48.
    *Why:* Debrief's import was bound by 2 Tantivy writer threads and a single-threaded catalogue, and regex was 38% of parse
    time; these were found by accident, and should be found by design.

## Thread priority (user, 2026-10-30)
52. **Work a person waits for never runs at an OS's starvable background level.** The starvable levels are macOS QoS
    `BACKGROUND` and process background (`PRIO_DARWIN_BG`, `taskpolicy -b`); on Windows `THREAD_MODE_BACKGROUND_BEGIN`,
    `PROCESS_MODE_BACKGROUND_BEGIN`, `IDLE_PRIORITY_CLASS`, `THREAD_PRIORITY_IDLE` and `THREAD_PRIORITY_LOWEST` (the
    background modes also drop I/O and memory priority to the lowest); on Linux `SCHED_IDLE` and the idle I/O class
    (`ioprio` `IOPRIO_CLASS_IDLE`). Lowered work that a caller may wait for (an analysis behind a report, a pool an
    import waits on through a lock, a download, anything a person started) uses the gentle level instead:
    `QOS_CLASS_UTILITY` on macOS, nice 10 on Linux, `THREAD_PRIORITY_BELOW_NORMAL` on Windows (Debrief's
    `debrief_core::threads::lower_priority`). A busy machine slows such work, never stops it. A starvable level is only
    for work nobody waits on (family ADR 0020's `Idle` class, ADR 0024's `Background` child processes), and each use
    says why with `// conformance: background-qos — <why nobody waits on it>` (check C54). *Why:* these levels run a
    thread only when the machine has nothing else to do (macOS `BACKGROUND` also keeps it on the efficiency cores);
    Debrief's mining pool was in it, so on a loaded Mac `debrief report` waited minutes for its analyses and timed out
    in CI (Debrief ADR 0052, amended 2026-10-30; lessons 2026-10-30).

## Monorepo (ADR 0014 §10; effective for an app from its import into `paddock/apps/<app>`, for Paddock from the first import)
Until an app is imported, rules 2, 3, 8, 9, 39 and 42 above apply to it as written. From its import, these replace them:
- **Rule 2:** every agent works in its own git worktree of `paddock` (sparse to its app where possible:
  `git sparse-checkout set --cone crates ui scripts docs apps/<app>`), with its own `CARGO_TARGET_DIR` per app and its own
  e2e port; the main checkout stays on `main` for the coordinator.
- **Rule 3:** the affected tests are what `scripts/affected.mjs` selects, plus the touched specs; otherwise unchanged
  (Playwright in Chromium only in a branch; the full suite, Chromium and WebKit, once on the combined result before a
  local build or release, plus CI).
- **Rule 8:** rules live once, in this file and the root `AGENTS.md`; no per-app copies. Every lesson still goes into
  [lessons.md](lessons.md); no `Co-Authored-By` or AI attribution anywhere.
- **Rule 9:** releases only from a `<app>-vX.Y.Z` tag in CI, never from a local machine; a local build for the user to
  try is fine but is never published. Never tag a `[skip ci]` commit.
- **Rules 39 and 42:** shared code is moved to `crates/` or `ui/` and adopted by the app **in the same change**; there is
  no publish-and-consume step. "Consume the published version" becomes "use the in-tree version".
- Rule 1 is about attention, load, CI capacity and shared-code conflicts, not repos (amended 2026-10-28: one release at a
  time; other apps only within those limits).
