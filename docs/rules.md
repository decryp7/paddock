# Paddock rules

Binding rules for every Paddock app and for everyone (people and agents) working on them. Each rule comes from a user
decision or a lesson; the reasons are in [lessons.md](lessons.md). Each app's `AGENTS.md` carries these rules; when a rule
changes here, every app's copy is updated in the same change.

The rules marked ER-n come from the generic engineering-playbook repository, which keeps only the generic rule and a short reason.
The Paddock incidents, run ids and ADR numbers behind each ER rule are in
[lessons.md](lessons.md#incidents-behind-the-engineering-playbook-er-1-to-er-51-moved-from-the-generic-rules-repo-2026-11-24).

A branch never picks a rule number (user, 2026-11-25): it writes a new rule as `NEW-<slug>.` in place of the number and cites it as
`rule NEW-<slug>`; the train integrator gives it the next free number when the train lands and replaces every citation. Numbers are
unique (C31).

## Way of working
1. **One release at a time.** Release-critical work comes first. Other apps may progress in parallel only while load and
   CI capacity allow, and only if they don't change shared code (`crates/`, `ui/`) the release depends on; tags stay
   serialized (amended twice on 2026-10-28; ER-1).
2. **Every agent works in its own git worktree** with its own `CARGO_TARGET_DIR` and its own e2e port; the main checkout
   stays on `main` for the coordinator.
3. **Tests in a branch: only the affected ones** (touched crates, touched specs, related Playwright files), with
   Playwright in Chromium only. The full suite, Chromium and WebKit, runs once on the combined result before a local
   build or release, plus CI. CI runs the foundation (`crates/`, `ui/`, tools) plus the active apps (the current release set); the nightly runs all apps. Doctests are off; a crate turns them on only if it is published for others or its docs become a real user guide (ER-7, 2026-11-24). Fix everything locally first; CI confirms, it is not where failures are found.
4. **When CI fails:** fix and re-run only the failed tests locally, push once; never push fix by fix to see what CI says.
   Lint and type-check the touched files before every push.
5. **Every bug fix starts with a failing regression test.** A test-only failure is fixed properly (deterministic, no
   sleeps or raised timeouts), never silently skipped. No limit (timeout, retry, wait, budget, tolerance, cap) is
   raised without a named, measured cause recorded next to it (ER-12); otherwise the cause is fixed. A new gate, CI job or
   check starts report-only and blocks only after its first green run on the real tree, recorded by run id where the
   gate is defined (ER-41). Its report-only phase is short: a report-only check, register or port registered after
   2026-11-24 carries an expiry date (`review_by` of its `[[report_only]]` entry in docs/gates.toml) at most 14 days
   later, and by then it is flipped to blocking, switched over (the old side deleted) or removed. C100 warns a week
   before; after the date it blocks release tags and the entry's own changes, and warns on any other push (user, 2026-11-24).
   Every report-only check, register and hook carries a review date; if it caught nothing real by then it is deleted, not extended, and extending needs the user (ER-41, 2026-11-24).
   A report-only gate reports in the job summary, a `::warning::` per finding and the terminal, with a test
   that a planted finding appears there (ER-41, user, 2026-11-24).
   A FLAKY test (fails, then passes on its one retry) blocks a landing only when the change touches the test itself or the
   source it exercises (its feature or module); see rule 50 (k) (ER-12, user, 2026-11-19).
   A reviewer of a bug fix runs a mutation check on it (user, 2026-10-30), in their own worktree: for Rust,
   `cargo mutants --in-diff <diff> -p <crate> --timeout-multiplier 3` with `CARGO_BUILD_JOBS=2` (install once with
   `cargo install cargo-mutants --locked`); for TypeScript, revert the fix and run the new test. The review reports whether
   the new test fails without the fix; a test that still passes proves nothing and is sent back. (A `cargo mutants
   --in-diff` CI job on the shared crates comes after the releases; see
   [lessons.md](lessons.md#a-bug-fix-review-runs-a-mutation-check-2026-10-30).)
6. **Tests never hard-code the app version;** they read it from its single source and check its shape.
7. **Tests never touch the user's machine:** no real launchd/Login Items/services, no installed apps, never the
   installed Google Chrome (pinned Chrome for Testing or an explicit env var only), no `~/Documents`, `~/Desktop`,
   `~/Downloads`, no real user data, no windows on the desktop, no processes left behind. Docker: only containers you
   created, each with `--rm` and CPU and memory limits (for example `--cpus 4 --memory 12g`). Agents never restart, stop
   or reconfigure shared infrastructure (Docker or OrbStack, CI runners, the test VM outside its lock); if it is down or
   out of resources, the agent stops and reports instead of fixing it (2026-11-11; ER-14). *Why (added 2026-11-11):* an
   uncapped test container ran OrbStack's VM out of memory; the agent then restarted OrbStack, which also restarts every
   container of the user's, and Docker was down for about 15 minutes.
    Fixtures recorded from live APIs or CI are scrubbed of names, emails, avatars and hostnames at capture; a check fails on real-looking values (ER-14; 2026-11-12). Paddock's check C110 does this.
8. **Every lesson goes into [lessons.md](lessons.md)**, every rule here; no `Co-Authored-By` or AI attribution anywhere.
    Hooks and gates are never bypassed: no `--no-verify`, `-n`, `LEFTHOOK=0` or `LEFTHOOK_EXCLUDE`; fix the failure or stop and report (ER-39; the worktree-guard hook refuses these for agents; 2026-11-12). Generated files (SVGs, lockfiles, synced copies) are regenerated from their source, never hand-edited, and CI checks they match (ER-40). Checks run on every change that could bring in their class, not only when their own files change; conformance runs on every change: CI plans its job for every change (`conformance always runs` in `./dev plan`), and the push gate runs the conformance row and the conformance tests on every push (ER-41; user, 2026-11-14).
9. **Releases only from a tag in CI**, never from a local machine; a local build for the user to try is fine but is never
   published. Never tag a `[skip ci]` commit.
    A tag or landing counts only a complete CI run on that exact commit, not a skipped run, a failed-only rerun or an in-place retry; `./dev ci verdict` is the one reader (ER-15; 2026-11-12).

## Long operations and performance
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

## User interface
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
    The coordinator proposes the redesign in the same reply that relays the report (a question to the user, or a start under
    an earlier approval), never ending a turn with an unproposed class (ER-21). Simpler first (2026-11-24): a repeat first asks whether the failing thing can be deleted or simplified, or common practice used; a new mechanism or ADR comes only when that cannot hold long term, and the proposal states what it removes. Before building anything, walk the necessity ladder (PRINCIPLES.md, KISS and YAGNI).
    Hooks that judge an agent's text (the recurring guard hook `.claude/hooks/recurring-guard.sh`, advisory) only warn; hooks
    that guard state (the cd guard, safe-remove) still block (user, 2026-11-12).
    A gate's "passed" is a value only the check can produce, and its tests show both a deleted check and a skipped gate fail (PRINCIPLES.md, robustness; 2026-11-12).

## Throughput (user, 2026-10-24)
41. **Scale parallel work to the machine.** The coordinator watches load and free disk continuously and keeps the
    machine busy but not overloaded: start more agents when load stays low, hold or stop starting them when it stays
    high (Mac mini, 14 cores: aim for a 1-minute load of about 6–16, at most ~5 heavy jobs, `CARGO_BUILD_JOBS=2` each),
    use the shared build cache (`sccache`) and copy-on-write target folders, and never let tests crowd out a release.
    Only one train's CI runs at a time on each shared runner class: `./dev ci dispatch` sends a train run at once and its first
    job waits (`runner-wait`, on Linux) while an older run holds the Mac or Windows runner it needs; other dispatches wait in
    the dispatcher. A run holds a class while it has an unfinished job on it; in a busy check, unknown counts as busy (a job
    whose label is unresolved, empty or of no known class counts its run by its workflow file); only an unreadable API fails
    open. At most the Linux slots less one (`conformance/ci/runner-slots.json`) train runs wait at once, so one Linux slot
    stays free (ER-24; 2026-11-12, mechanism 2026-11-15; first live run: pending).

## Sharing (user, 2026-10-24)
42. **Share first.** Before building anything in an app, check Paddock's shared crates and packages (crates/README.md,
    ui/README.md, docs/plans/) for it. If it exists, use it; if it is missing, generic, or another app has or will need the
    same thing (rule 39), build it in Paddock and consume the published version instead of writing an app copy. Every
    work brief includes this check; every app move and every few releases a survey looks for copies that drifted apart
    and turns them into shared work packages. App-specific domain logic stays in the app.
    A value that another module decides (a size, a threshold, a count, a layout verdict) is read from that module, by a
    call or an exported constant, and never re-typed. Reviews check for it (ER-22; user, 2026-11-17). *Why:* Debrief's
    Explorer width constant copied the layout plan's rule and caused a 920-934 px jump, and an earlier 51 px placeholder
    had copied the filter bar's height.

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

## Test isolation (user, 2026-10-29)
49. **Specs never wait a fixed number of event-loop turns and never leave a patch behind.** Specs of one bundle share a
    page (the Angular builder runs Vitest with `isolate: false`), so: (a) wait on the condition, never on a count of
    turns or timers: Debrief's `openDialog()` (`testing/helpers.ts`) waits
    with `vi.waitFor` for a lazily loaded dialog and fails when none opens; (b) an optional chain never performs a
    test's action (no `stack().at(-1)?.ref.close()`): a missing thing must fail the test; (c) a patch on a prototype is
    a `vi.spyOn`, put back by `restoreMocks` and by a global `afterEach(vi.restoreAllMocks)`, never an assignment,
    `Object.defineProperty` or `Object.assign` on it. Once that branch lands, Debrief's ESLint (`no-restricted-syntax`
    for specs) enforces (b) and (c); the other apps follow. *Why:* a fixed wait and a leaked patch each made a spec
    fail now and then, only in the full run
    ([lessons.md](lessons.md#test-isolation-debrief-2026-10-29)).
    (d) A wait or assertion over a set of elements requires the set to be non-empty, or names why empty is expected (an `every()`
    over nothing passes), and a layout check counts only when two reads agree (ER-12; 2026-11-16: Dyno's layout check passed on
    zero rows and failed 31 of 240 runs under the pinned clock).
    (e) A claim that a timing-sensitive spec passes or is stable states its runs, workers and engines (or OS). An e2e timing fix
    needs at least 20 runs per engine at 4 workers, or more when the measured failure rate requires it; a bare "N of N green" is
    rejected in review (ER-28, user, 2026-11-17). A speed claim made on a shared machine compares CPU time or interleaved A/B runs and states the load, never elapsed time alone (ER-28, user, 2026-11-24). *Why:* on 2026-11-16 a Debrief live-tail fix was reported "3 of 3" and was 3 of 7
    red, and "12 of 12" was 4 of 150 red (`fix/debrief-events-grid-live-tail`, `fix/debrief-live-tail-adr`).

## CI economy (user, 2026-10-29)
50. **CI runs only what a change needs, once.**
    (a) At most one CI run per branch head. Before dispatching, cancel any queued run on the same branch with an older head.
    The branch's one run is the trial run: `./dev ci dispatch ci.yml --trial <branch>` merges the branch head onto the open train
    (origin/main when none is open) in its own clone, reports a conflict with its files instead of dispatching, and dispatches
    with `base` = the target's sha, so the one run sees the combined tree (ADR 0098 WP-4; report-only for the reviewer until
    its first green trial run is recorded in the ADR).
    (b) Do not dispatch while review fixes are pending. Dispatch once after the last fix, and do not re-dispatch just to
    refresh against main; merge main right before landing.
    (c) A branch already merged into another branch gets no CI of its own.
    (d) Docs-only changes land after the local conformance checks, without waiting for CI.
    (e) Branch runs use the affected plan (rule 3); the full gates run once on main or at release.
    (f) is reserved for the push check (ADR 0049 WP-1, D2).
    (g) Before landing a branch (a merge into main), the integrator lints its commits locally:
    `node conformance/bin/paddock-conformance.mjs commit-lint --range origin/main..<branch>`. A push to main lints only what main
    itself gets (`--first-parent`), so this is where a branch's own subjects and trailers are checked; C23 is the backstop.
    (h) Local checks and CI overlap; a local failure cancels the run (ER-53). Before dispatch, run the fast static gates locally,
    in parallel and bounded by the machine's load: `./dev check --pre-push` (the pre-push hook runs it): `cargo fmt --check`,
    clippy on the host and, for Windows, `cargo xwin clippy --target x86_64-pc-windows-msvc --workspace --all-targets -- -D warnings`
    (an import or helper used only by a unix-gated test is dead code there; ADR 0021, lessons.md "Windows-only assumptions in
    tests"), conformance with ci.yml's own list and the conformance tests (`cd conformance && npm test`, both on every push, ER-41;
    a flaky conformance test that blocks a push is fixed at once under ER-12, never skipped or bypassed, ER-39),
    commit-lint (rule 50g) and lockfiles with `--locked`; when a Cargo workspace
    has a changed file of its own, its Clippy on the host and (Windows) through xwin; Linux Clippy is CI's Linux job, since no VM or container starts on this Mac (user 2026-11-26, rule NEW-no-local-vm). A gate that cannot run (no
    cargo-xwin) is "not measured", exit 6, never a pass. It checks every commit the remote
    lacks (`git rev-list <tip> --not --remotes`), measured from the nearest commit of the remote the push contains (the header says
    which and why; never a local branch), and its 15 minutes count run time only; time queued for the heavy slot behind other
    agents has its own allowance of 30 min per push and is reported apart (ADR 0102 and its 2026-11-15 amendment). Lint and type-check of the touched
    TS files stay by hand until WP-3. Then dispatch with `./dev`
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
    full green run. `ci.yml` and `debrief.yml` take this as the dispatch input `since_run` = the failed run's number (`./dev
    plan --since-run <id>` shows the plan; a full `debrief.yml` run's failures also need `bundles: true`); the full run before
    landing (ER-10) is a plain dispatch without `since_run`.
    *Why:* CI runners are the scarcest resource, and every wasted run delays a release.
    (j) Every merge into a train or integration branch runs at merge time the checks that fail only on combined code (workspace-hack, publish dry run, conformance), not first in the full run (ER-8; 2026-11-12).
    (k) The member gate (ADR 0098): the integrator runs `./dev merge check --gates <merge>` after each member's merge, which runs
    the fast gates of `./dev check --pre-push` on the combined tree with `--base <merge>^1`. A member that fails them is taken
    out (the train is reset to `<merge>^1` in the integrator's own worktree) and sent back to its author with the failing
    rows; it is not fixed on the train. The gate is report-only (printed, exit unchanged) until its first green run on a real
    train is recorded in ADR 0098, and sends members back from then on. For a failed train run, `./dev ci triage <run> --members
    <train-base>` names the members whose change reaches each new failure (the queue's own rule). The member gate also runs the touched
    workspaces' nextest `ci` profile on macOS and their OS-conditional code in the Linux CI container (user, 2026-11-25); the full
    suite stays once per train.
    The train's sequence (user, 2026-11-16): (1) each member's one short review runs at the same time as the train's CI, not
    before the merge; a P1 cancels the train's run and pulls the member, so a review never delays a green train. (2) The next
    train is assembled during the current train's CI: its integrator merges members on top of the running train's head, runs
    all local gates, and re-merges the running train's final head when it lands, so the next CI starts at once. (3) The
    train dispatches an app's release gate only when the plan marks it (rule 54 (a2)), and with `gates=false` once the counting
    full `ci.yml` run is green on that exact commit (rule 54 (a3)).
    (4) A test that fails and then passes on its one retry (FLAKY) blocks the landing only when the train's change touches the test
    itself or the source it exercises (its feature or module: the files it drives, not any file of the same app package or crate;
    ER-12, user, 2026-11-19). Otherwise the landing goes ahead: the flake is recorded with its log as a
    fix item and fixed in the next train or batch, with a failing reproduction first (rule 5). A test that fails both tries always
    blocks, and a retry never turns the run itself green: the report still marks it FLAKY. *Why (ER-12):* on 2026-11-16 two old
    races in unchanged shared crates (run 8174: `paddock-ipc` endings, plugin-host ETXTBSY) held train 12 for hours. *Why:* release gates cost about 77 minutes of the Mac runner
    on train 12 (runs 8164, 8165, 8167, 8169); the review before the merge and the serial assembly after the green run were
    waits with nothing running (their minutes are not yet measured; the landing report records them from the next train).
    The gate that decides on a branch runs on the same kind of machine as CI (the real runners, as merge queues and merge trains
    do); a run on another architecture, under emulation or as another user counts only as not measured, or as a finding only when
    compared with the base's own result on that machine (ER-53; 2026-11-16: the Windows-on-Arm VM and an amd64 container failed 22
    paddock-dev tests on a base the real runner passed).
    A member the train pulled out with a revert keeps its work when its author catches up: before `git merge <train>`, run
    `./dev merge sync-member <train>` (read-only); it prints a `git revert <sha>` for each live train revert of a merge of this
    member's own branch, to run after the merge (git's howto "revert a faulty merge": a plain merge of the train would delete the
    member's work again, as the revert is the newer change; three times, the last c156ba72a), and warns about reverts of other
    branches' merges the member holds (re-base on the train, or land that branch first). The merge check's `--write-reverted
    <file>` then writes the `Reverted:` lines of that re-land for the train side, only for a path with no other finding that the
    target has not changed since the revert; every other finding stays a person's. `./dev merge check` also warns, report-only
    (ER-41), about a member that does not contain the train's base (the newest commit of its base line the train has merged: the
    second parent of the newest re-merge of an earlier train on the train's own first-parent line, else main's merge base); other
    members merged ahead of it are not required.
    **git_env (ER-47 policy).** paddock-dev starts every git through `git_env::git`, which removes the variables that select a
    repository (`GIT_DIR`, `GIT_WORK_TREE`, `GIT_INDEX_FILE`, `GIT_COMMON_DIR`, `GIT_OBJECT_DIRECTORY` and the rest of
    `GIT_REPO_VARS`) because the tool always names its repository with `git -C <folder>`: git sets `GIT_DIR` for a hook, and it
    would otherwise override that folder. Everything else is inherited. A user's deliberate `GIT_DIR` therefore has no effect on
    paddock-dev: point it at a repository with `--root` or the current folder. A source test refuses a bare `command("git")`.

## Performance analysis (user, 2026-10-29)
51. **Retired 2026-11-12 (user): merged into rule 13** (more parallelism, or a cheaper operation).

## Thread priority (user, 2026-10-30)
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

## Process identity (user, 2026-10-30)
53. **Never act on a reusable OS name read earlier.** A process id, a port or a temp path read earlier may name something
    else by the time it is used. Hold the thing itself (a process handle, a job object, a bound listener, an open file)
    or verify its identity immediately before acting (for a process: its pid plus its start time,
    `paddock_proc::ProcessId`). A tree is followed by age (a child never started before its parent) or by job
    membership, never by parent pids alone. Kills go through `paddock_proc` (`kill_tree`, `end`, `ask`, `terminate`,
    `Supervised`). Clippy's `disallowed-methods` (lint/clippy.toml: `libc::kill`, `libc::killpg`, nix's `kill`/`killpg`,
    sysinfo's `Process::kill`/`kill_with`) and conformance check C53 (`taskkill`, kill commands, `Stop-Process`,
    `TerminateProcess`, `TerminateJobObject`, open ADR 0033 follow-ups) enforce it; for an agent's own Bash commands the
    blocking hook `.claude/hooks/kill-guard.sh` does, once it is wired in the main checkout's `.claude/settings.json` (`pkill`,
    `killall`, `fuser -k`, `xargs kill` and a `kill` of a pid it cannot prove is the agent's own are refused). ADR
    [0033](adr/0033-processes-by-identity-never-by-a-bare-pid.md); ADR [0023](adr/0023-tests-never-free-a-port-they-probe.md)
    is the same rule for ports.
    A discovery file, a file that names a pid or a port (Dyno's `agent.json`), is only a hint: act on it only after a held
    lock or a live connection proves the process is there, and remove it only if it is still the same file (same pid and
    start time), never because it was the file read earlier.
    *Why:* Windows hands a freed pid on at once. `kill_tree` killed the pids of a stale snapshot, which can end another
    test's Chrome helper; the same mistake was in four shapes in five apps. (It was suspected of causing the Windows
    `ERR_ABORTED` failures; the suspected cause was disproved by run 7158; the flake has another cause, ADR 0033 WP-4.)
    Dyno's stale `agent.json` made the app say "Agent stopped" and loop on exit 3 for a process that was gone or replaced
    ([lessons.md](lessons.md#a-discovery-file-is-a-hint-never-proof-2026-10-30-dyno)).

## Release path (user, 2026-10-30)
54. **Every change that touches an app's release path builds that app's installers before it lands on main, once per
    train or batch that touches it (user, 2026-11-12), not once per branch.** The release path is the release workflow,
    the shared app-release workflow, the bundle and prepare steps, the Tauri config and the app's imports of shared
    packages.
    (a0) Narrowed (user, 2026-11-23): a check a release job runs (a `run:` step: the release preflight, a conformance check or
    a check script) is not the release path. A change that reaches only such a step selects the app's smoke set and its
    `checks` phase, not the installers (ADR 0117's `gate` reach; `docs/repo-graph.toml` `[ci] release_gate_phases`). The
    installers stay for the bundle, sign, notarise and publish steps (the steps that use an action), the app's code, the
    Tauri config and tags. A smoke run never counts as a complete verdict. Rule 87 points here.
    (a) The batch dispatches `<app>-release.yml` with gates on and the installers (bundle) built; passing tests alone is
    not enough.
    (a2) A train dispatches an app's release gate only when the plan (`./dev plan`, over the train's
    diff against main) marks that app's release workflow or release path (user, 2026-11-16). Otherwise it skips the gate and
    records the plan output in the landing report. The counting full `ci.yml` run always runs. This is also how rule 50's train
    sequence reads. *Why:* each unaffected release gate cost up to 34 minutes of the single Mac runner per train (train 12:
    runs 8164, 8165, 8167, 8169, about 77 minutes in total).
    (a3) When the counting full `ci.yml` run is green on the exact commit, the affected release gates run with `gates=false`:
    they build and check the installers, which is what rule 54 needs, and do not rerun the tests `ci.yml` just ran on that sha.
    If `ci.yml` is not yet green on that sha, `gates=true`. Checked 2026-11-16 in the workflows: in `dyno-`, `pitwall-`,
    `pitcrew-` and `spotter-release.yml` the `installers` job does not need the `gates` job, and calls `app-release.yml`
    with the same inputs either way, so the bundle check, the Windows bundle and installer checks and the macOS verify
    (sidecars started with `--version`) run with `gates=false`. The version and notes checks of `app-release.yml` run only on a
    tag, so a dispatch never ran them with `gates` on or off. `shell-demo-release.yml` and `workbench-demo-release.yml` have no
    `gates` input (their `gates` job always runs and `bundle` needs it), so (a3) does not apply to them.
    (b) Partly enforced: the ci-actions test (added on fix/dyno-bundle-ui) checks that every release workflow whose app
    imports `@decryp7/paddock-ui` builds the UI.
    (c) Amendment of 2026-11-09 (ADR 0080 D13, accepted by the user): the release workflow dispatched for this rule
    skips its gates only when a green `ci.yml` run on the same commit covered every gate it would run, compared job by
    job (the affected plan and ADR 0080 D12 can leave legs out of `ci.yml`, and the release workflow then runs those
    legs, rule 71); the installers are always built.
    *Why:* Dyno's release workflow had no `prepare: npm run build -w ui`. Tests passed, but every installer build failed
    on a clean runner; a leftover `ui/dist` had hidden it.

## Copies (user, 2026-10-31; ADR 0034, rule 40)
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
    (e) Enforced by conformance check C58 (ADR 0034 WP-1; not built, so the steward checks the register by hand). A register-only change is docs-only
    and gets no CI run (rule 50), so the steward checks the register by hand before it lands. Once C58 exists, batches
    run `copies --range <base>..HEAD` before pushing, and the steward runs `copies --tag <tag>` before creating a tag.
    Until C58 exists, the reviewer and the steward check the register by hand.
    *Why:* launchd's job report existed four times. The shared one already skipped nested blocks; Dyno's copy did not
    and replaced its healthy agent at every login (Dyno 1.0 blocker B4). The Windows network-sandbox fix of the browser
    driver sat in the one copy no app used, and the EBADF fix had to be made in two IPC endpoints.

## Release checks (user, 2026-11-05; ER-33)
71. **Retired 2026-11-12 (user): merged into ER-33** (a release check counts only if its precondition was verified and its probe has shown it can see the thing; otherwise `not measured`).
    Every new check, gate, test assertion over a signal, and shared wait helper lands with a positive control: a test that
    removes or hides the thing it watches (the signal, element, attribute or job step) and shows it goes red. A review
    rejects one that has none (ER-33, user, 2026-11-17).

## Design decisions (user, 2026-11-11; ER-37)
76. **Measure a design against common practice before paying for it.** Before an ADR or design is accepted, it states how
    comparable, well-known software handles the same problem (for example Visual Studio, VS Code, Chrome, Docker, Homebrew,
    git, the OS vendor's own tools) and what the platform vendor documents as the standard (Microsoft, Apple, Linux
    distributions), with sources. It then states the cost in agent-days, set against that baseline. A design that goes
    beyond common practice says why this project needs more, and the cheaper common-practice option is always offered as a
    decision. Personal, single-user tools default to common practice.
    A design, mechanism or fix costing more than 1 agent-day beyond the common-practice option needs the user's yes, with
    both costs shown; a standing "build everything" OK does not cover it. Each release plan states its budget in raw
    agent-days when its scope is agreed, and growth beyond 20% goes back to the user. Each release gets one cut review, at
    scope freeze, by one reviewer, against this rule (ER-37, amended 2026-11-12). One in, one out (2026-11-24): a new check, register, hook, gate or ADR mechanism removes or replaces something of similar cost, or names a recurring real failure and the comparison; a proposal states what it adds and removes.
    *Why:* on 2026-11-11 a 23-31-day elevated-helper design for Debrief went through four review rounds before the user
    compared it with Visual Studio and VS Code and chose a half-day risk warning instead; several accepted process ADRs
    (signed approvals, decision registers, extra gates) were then cut as more than a personal tool needs.
    On 2026-11-12 a cut review found Debrief 1.2 grown to about 656 raw days and Pitcrew's 1-2 day caller check in its 8th
    review round.

## Privileged services (user, 2026-11-11; ER-38)
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

## Release builds (user, 2026-11-10 and 2026-11-12; ADR 0085)
78. **A tag run reads no state that a non-tag run can write.** It builds from folders it creates, without a compile cache,
    with signing keys only after the install step, and with bundler tools fetched from pinned URLs and SHA-256 values; a
    check fails it otherwise. The real updater key reaches a step only on the app's release tag (`refs/tags/<app>-v`), and
    inside it `tauri build` and the pre-sign `sign` process (Dyno's macOS helper) only, in their environment, never in a command line, `npm ci`, a prepare command or the app's
    `beforeBuildCommand`; a dispatch, nightly or branch run signs with a throw-away key made in the job, and a tag never
    does. Enforced today, and nothing more:
    (a) `conformance/ci/updater-key.sh`: in CI it fails a private key on a run that is not a tag, and a tag that has the
    public key but no private key; it hands the key to `tauri build` by `export` and `exec` only.
    (b) `conformance/test/updater-key-tags.test.mjs`: every workflow step gates the key with the release tag prefix, Debrief's
    bundle step takes the key before any `npm ci`, and the bundle jobs of `app-release.yml` and `debrief.yml` skip the kept
    npm and Angular caches and Debrief's kept Windows `node_modules` on a tag (`conformance/ci/tag-node-caches.sh`).
    (c) `conformance/ci/before-build.mjs`: the `beforeBuildCommand` runs before `tauri build` without the key, and `tauri build`
    gets it emptied; a `beforeBundleCommand` is refused.
    (d) `conformance/ci/tauri-tools.mjs`: every tool Tauri would download for an NSIS or AppImage bundle is pinned, a tag
    reads no kept tool cache, and `check` fails a tag on any unpinned or changed file in `<build folder>/.tauri`.
    (e) `conformance/release/updater-key-check.mjs` (every `.sig` verifies against the build's one public key) and, on a tag,
    `check-per-machine-installer.mjs` (`paddock-setup.exe`'s `key_fingerprint` is that key).
    Not yet enforced: the build folder, `CARGO_HOME`, the compile cache, the kept clone of a host runner and cargo build
    scripts that still run with the key inside `tauri build`; they follow with release mode (ADR 0085 WP-1 to WP-3).
    *Why:* on 2026-11-10 synthetic experiments showed that a branch run can poison what a later tag run reads (an sccache
    entry, a kept build folder, the cargo registry), and every rule 54 dispatch had given the updater key to `npm ci`
    (`tauri-bundle.sh:55`), so every install script of every branch could read the key that signs updates.

## Declared data (user, 2026-11-13; ADR 0098)
79. **Tests read declared data through its accessor** (ER-42). A test, lint list or registry entry that needs data another part of the
    repository declares (a workflow's command line, a manifest's dependencies, a register's rows, where a file lives) reads it
    through the one accessor that owns that data (`ci_run::contract::conformance_args`, `cargo metadata`, the register's parser, a
    constant the owning module exports), never by grepping or slicing that file's text. A source audit that must read another
    crate's code names the file through a path the owner exports and ends its span at an item boundary, not at a comment it
    remembers. A hand list that must equal generated data (heavy's `inputs.txt`) is generated, or its test prints the
    regenerate command (ER-40). Not linted: review and the train's test run cover it (the check that flagged a regular expression
    over `.gitea/workflows/*.yml` was dropped on 2026-11-25: its report-only phase found only fixture text).
    *Why:* on train 8 (2026-11-13) eight fixes after the merges were combination failures; `bddd2947c` was a second reader of
    data that already had an accessor (`./dev ci conformance-args` now), `0308201b4` a span ended at a comment the author
    remembered, `11c365fc4` a path into another area's layout, `be69a31f6` a hand list that had to equal `cargo metadata`.

## Waiting, fixes, checks, lists and environments (user, 2026-11-15 and 2026-11-16; ER-43 to ER-47)
Status is as of train 14 planning: "planned" is not built, "in progress" is on a branch. Rules 80 to 84 map the engineering rules to Paddock's mechanisms.
80. **A waiter never holds the capacity the work it waits for needs** (ER-43). Paddock: `./dev wait` and the heavy-slot queue take no slot for the wait; the waiter guard (duplicate waits, grep polling) and the scheduled-runs-yield rule follow. Status: planned, train 14.
81. **A complete fix, not a quick one** (ER-44). The principle (user, 2026-11-24): the minimum simplest complete fix, and a redesign only if it is necessary for the long term; rules 76, 83 and ER-21, ER-36, ER-41 enforce it. A review's or incident's cause, its fix and its copies in `docs/known-copies.toml` (rule 55) land in one round; ask the user only above 1 agent-day beyond common practice (rule 76). Status: in force.
82. **Checks have no side effects** (ER-45). Paddock: launcher probes and `setup-rust` set `RUSTUP_AUTO_INSTALL=0`, shell checks print the cause before exiting under `set -e`, and a check's positive control runs on in-memory data . Container runs go through `tools/linux-container.sh` only (ER-14); tests use `StandIn` programs, never installed apps or `~/.rustup`. Status: in progress, train 14.
83. **Lists only shrink; goldens hold no machine** (ER-46). Paddock: planned growth checks compare each list with the merge base and fail on growth without the user's approval (one each for the exceptions list, the allow list and the baselines; the ids are reserved with `./dev check new-id`); goldens are normalised (uid, names, paths, sizes) and the e2e suites use the fixed clock. Status: the three checks are planned (report-only first, ER-41); fixed e2e clock in progress.
84. **Children inherit the environment** (ER-47). Paddock: production children inherit: `paddock_proc::clean_env` keeps its kept list and every restricting variable the caller has, as the caller has it. `TestCommand` is an allow-list on purpose (ADR 0100): it clears only through `paddock_env::clear_env_pinned_from` and keeps a restricting variable only in a restricting value (a developer's `PADDOCK_DEV_DOWNLOAD=1` is dropped), and sets `RUSTUP_AUTO_INSTALL=0` when the caller's value does not restrict. Clearing needs a named policy with a kept list and never strips a restricting value. The restricting variables are the one typed list `paddock_env::RESTRICTING` (`RUSTUP_AUTO_INSTALL=0`, `CARGO_NET_OFFLINE=true|1`, `PADDOCK_DEV_DOWNLOAD=0`, `PADDOCK_TOOLCHAIN_FAILED=1`), and the only function that clears a child's environment is `paddock_env::clear_env_from` and its pinned twin: Clippy forbids `Command::env_clear` elsewhere (ADR 0100, ER-47 section). Status: in progress, train 14.

Amendments of the same date, no new number: the member gate runs each member's affected tests before it joins a train (ER-9): the full nextest `ci` profile of every touched Cargo workspace on macOS, its reached OS-conditional code (the `hostTests` placement derived from the code) also in the Linux CI container, and the guardian's acting tests as uid 65534 when the guardian changed; Windows tests run only on CI's own runner, with no separate Windows member check (ER-52, 2026-11-24; user, 2026-11-25); agents start on the 15-minute load and CPU idle, count container runs and run at most 5 builders while containers run (ER-24); agents never start the user's installed apps or touch `~/.rustup` and `~/.cargo` (ER-14).

## Bulk APIs (user, 2026-11-18; ER-20, PRINCIPLES "Bulk APIs")
85. **Bulk APIs: one call for many items** (ER-20; PRINCIPLES "Bulk APIs"; ADR 0108). A function, Tauri command, port method, IPC message or HTTP endpoint that works on items takes many at once (a slice, an iterator, a stream) and returns a result per item, so a caller with N items makes one call; a single-item form, where one is needed, is a thin wrapper over the bulk one; one bad item does not fail the batch unless the operation is atomic by design; large inputs are chunked or paged (ADR 0018, bounded loading). Work over many items is one bulk call or one long-lived process or connection: one SQL statement or one transaction, one bulk IPC request, one `git cat-file --batch` reader instead of a git start per object, an in-process library. A loop that calls a command, port, process, query or endpoint per item is a review finding unless the code says why (`// per-item: <reason>`). A command's count of process starts, statements or invokes is a budget a test guards: each bulk command lands with a test that N items cost at most K, failing first with the per-item code. Status: rule and review line in force; the shared batch git reader (`crates/paddock-git`) and the stdin-taking bounded run in paddock-proc in progress on `perf/git-heavy-tests-2`; the counting seams, the per-item result type (ADR 0108). *Why:* on 2026-11-18 a git start cost about 45 ms on the Windows runner against about 6 ms on the Mac and the merge check started 33 to 75 gits per check (train 14); the inventory of that day found 65 more places across every app, the shared crates and the dev tools (docs/research/2026-11-18-bulk-api-inventory.md), after six earlier instances fixed one by one (ADR 0108).
97. **Start, hold, watch, end** (ER-51; ADR [0112](adr/0112-job-owns-its-processes.md)). Whoever starts a process, a CI run, a container or a slot keeps the handle the start gave back (`paddock_proc::ProcessId`, the run id, the container id, the holder token), waits on it with a bound and ends it through it; nothing is found again by name or by search (`pgrep`, `pkill`, `ps | grep`, "the newest run"). Every Paddock CI job is a fixture: the shared job action `./.gitea/actions/job` prepares (health, slot, the job's process ledger, the job's one folder for temp, logs and outputs) and its post step cleans up on success, failure and cancel (ends the ledger's processes by identity, releases the slot, writes the job summary). Nothing writes outside the job's folder except declared outputs and the shared caches named in `kept-folders.txt`. Status: rule in force for new jobs and scripts; the job action, the fixture check and the handle check (both report-only first, ER-41) are planned (ADR 0112 WP-4 to WP-6). *Why:* on 2026-11-20 a host job left `vm-lock.sh` running for four days and leaked test folders filled the Mac runner's temp folder (runs 8334, 8341, 8346), after the slot-holder leak of run 8099 (82d5ee496); macOS hides the environment of Apple programs, so cleanup by marker missed the shell scripts.

## CI selection and test runs (user, 2026-11-23; ADR 0117; ER-9, ER-32, ER-49, ER-52)
86. **Only setup-only actions declared in `docs/repo-graph.toml` are plumbing.** A change to an action, script or doc that the graph
    does not declare as plumbing reaches the projects that use it, and an unowned or unresolvable reference selects everything (fail
    closed, ADR 0117). Nothing is plumbing by its folder or name. *Why:* `.gitea/actions/README.md` was on the planner's global list
    and train 20 spent six full rounds (runs 8610, 8635, 8647, about 700 to 730 job-minutes each) on plumbing changes.
87. **A change to a conformance check, or to a check a release job runs, runs the app's smoke set and its checks phase, not the
    installers** (user, 2026-11-23). The text is rule 54 (a0), the one copy.
88. **Removing an OS from a test needs the user's approval.** The record is `docs/os-removals.toml`; growth of that register is a
    finding that no waiver clears (rule 83; ADR 0117). *Why:* OS-scoped tests (ER-52) cut Windows time by 46%, and an OS
    dropped without a reason is a hole in the net.
89. **Every test run goes through the bounded runner** (ER-32, ER-51; amended (user, 2026-11-24)): `cargo nextest run` with the
    workspace's `.config/nextest.toml`, which gives every test a per-test timeout (`slow-timeout` with `terminate-after`) and one
    process per test, as common practice does (nextest, Go's `go test -timeout`, Bazel's test sizes). The guard hook refuses plain
    `cargo test` (`cargo test --doc` and `--no-run` stay allowed), and a test in tools/paddock-dev reads every CI line through the CI
    model and fails on one. Progress comes from the job record (ADR 0118) and ADR 0119's required bounds; no watchdog wrapper
    runs around nextest. A helper a test leaves running ends with its test (`paddock_env::lifeline`), and a leftover child is ended by
    the handle its start gave back. *Why:* on 2026-11-22/23 `worktree::list_clean` hung 3.5 h and `ci_isolation` 18 min under plain
    `cargo test --test it`: one test had moved the shared process into a temporary folder that was then removed, so every later git
    start read the whole temp folder; and test-spawned `ci slot hold` processes were orphaned four times.
90. **A landing may rest on a narrowed run** (ER-15, user, 2026-11-23): a smoke set or the affected tests that ran exactly the
    selection the landing's own diff planned. Such a run counts for that landing only: it is never complete for a tag or a
    release, never the base for later planning, and the nightly still runs everything. *Why:* in ADR 0117's review round 2,
    narrowed runs were never complete, so a train whose change needed only a smoke set or its affected tests could never land.

## Test-only hooks (user, 2026-11-25)
99. **Test-only hooks compile only for tests.** An injected clock, gate, fake, test setter or `for_tests` constructor is behind
    `#[cfg(any(test, feature = "test-support"))]`, and `test-support` (ADR 0073's name, the one name for this feature) is
    enabled only from `[dev-dependencies]` (a self dev-dependency for a crate's own integration tests) or a test target's
    `required-features`; production wiring that needs the same seam calls a crate-private function, never the public hook. C170
    (`./dev check test-features`, in the conformance job on every change it plans) proves that no released binary, each app's
    Tauri package, every `bundle.externalBin` and `paddock-setup`, is built with a test feature (`cargo tree -e normal`; it also
    flags a stray `testing`; report-only until its first green run, ER-41). *Why:* on 2026-11-25 `paddock_ipc::Host::with_clock`
    was public with no gate, and an ADR draft copied that pattern for the privileged dyno-helper daemon; a shipped hook is an
    API anyone linking the crate can call.

## Monorepo (ADR 0014 §10; effective for an app from its import into `paddock/apps/<app>`, for Paddock from the first import)
Until an app is imported, rules 2, 3, 8, 9, 39 and 42 above apply to it as written. From its import, these replace them:
- **Rule 2:** every agent works in its own git worktree of `paddock` (a full checkout: `./dev worktree add --profile gates`,
  the default; sparse only for a `--detach` review copy), with its own `CARGO_TARGET_DIR` per app and its own e2e port; the
  main checkout stays on `main` for the coordinator.
- **Rule 3:** the affected tests are what `./dev plan` selects, plus the touched specs; otherwise unchanged
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
98. **In a repository with several products, CI failures in code an app does not ship do not block its release** (was ER-31, moved here from the generic rules 2026-11-24). Judge failing tests and skipped steps, not jobs.
    (a) Tag the app in CI on its release branch's tested commit, and land the branch on main afterwards, when every failing test is in other products' own code or in shared code the app does not depend on, and every step a failure skipped has been re-run green or is shown not to touch the app's dependency set. List the dependency set from the lockfile or the package manager, and from the app's imports of shared packages where the manager cannot see them.
    (b) These always block: failures in shared code the app ships; an aggregate coverage failure, until a per-crate gate shows the failing crates are not the app's; another aggregate gate (dependency audit) unless its output clears the app's crates; and anything on the release path (the build and publish steps and the shared actions).
    (c) ER-8, ER-15 and ER-16 still hold: the full suite ran to completion on the tagged tree, the tag is made for CI to build from and never on a `[skip ci]` commit, and it is the commit the gates tested.
    (d) The landing needs the normal green gates and is a merge or fast-forward, never a rebase or squash, so the tag is an ancestor of main (later release notes read the history from the previous tag). Fixes for other products go on their own line, not on the release branch.
    *Why:* Dyno 1.1.0 was correctly blocked by Paddock run 7485 (mostly code Dyno ships). Run 7489 failed in the shell-demo's i18n check (outside Dyno) and in aggregate coverage; the coverage gate printed no per-crate output and a crate Dyno depends on was in it, so that blocked under (b) until the gate was fixed and a full run was green. The failures also skipped the dependency audit and several ui steps, which (a) asks to be cleared.

NEW-no-local-vm. **Local gates never start a VM or a container on a developer's Mac** (user, 2026-11-26). Linux checks run as CI's Linux jobs on the Linux runner, Windows checks on the Windows runner. The pre-push and member gate have no Linux row (`clippy-linux`, `tests-linux`, `--warm-linux` are removed; the register names them CI-owned), and conformance C-NEW (report-only, review 2026-12-09) flags a docker, podman, nerdctl, orb, orbctl, vmrun or `linux-container.sh` call in local gate code. *Why:* the container rows were the Mac's largest load, and the user decided on 2026-11-26 that no VM runs on the Mac. Status: in progress.
