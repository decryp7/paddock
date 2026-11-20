# The cheapest tests that keep their quality: selection, WebKit, duplicates and mechanics (2026-11-20)

Research note. Question (user, 2026-11-20): make Paddock's tests as cheap as possible without losing quality. Read-only on
product code: nothing was built, no test was run, no CI run was dispatched; the data are CI job logs already on the server,
the affected planner run over committed diffs, and the code at `origin/main`.

What stays, whatever is chosen here: branches run the affected tests and the full suite, Chromium and WebKit, runs once on
the combined result before landing (rule 3, ER-7, ER-8); no test is skipped silently and no wait is added (ER-12); a check
counts only if it can see its target (ER-33); quality is not traded for speed (ER-36).

Builds on, and does not repeat:
[2026-11-20-ci-job-cost.md](https://dev.decryptology.net/decryp7/paddock/src/branch/docs/ci-job-cost-research/docs/research/2026-11-20-ci-job-cost.md)
(per-job and per-step costs; Playwright is 56% of Linux job-minutes; Debrief WebKit 40% of all Playwright test-seconds) and
[2026-11-20-ci-speed.md](https://dev.decryptology.net/decryp7/paddock/src/branch/docs/ci-speed-research/docs/research/2026-11-20-ci-speed.md)
(where a train's wall time goes). Items already decided or being built are taken as given and not counted again: staged CI,
the fix ladder (fix rounds rerun only failed tests), the shared sccache, the Linux CPU share, D4 (no main re-run), `./dev`
and `ui/dist` built once, e2e against one production build, Dyno page-layout one load per page.

**Heads read** (ADR 0091), fetched 2026-11-20 01:45 UTC and re-read before the commit (see the end): `origin/main`
e0fd0f511991 (this branch's base), `origin/docs/ci-job-cost-research` 44d30408bd10, `origin/docs/ci-speed-research`
93bbd80637b3. `ci/staged-fast-then-long` and `feat/train-fix-ladder` were **not on origin**; what this note says about them
comes from the brief.

Every number is in the results block
[benchmarks.md, "Cheapest tests"](../benchmarks.md#cheapest-tests-selection-webkit-duplicates-2026-11-20-docscheapest-tests-research)
(rule 48). Raw data and scripts: `/Volumes/vault-dev/dev-targets/scratch/cheapest-tests/` (not committed).

## 0. Summary

| # | Change | Saving | Agent-days | Quality risk | Safety net kept |
|---|---|---|---|---|---|
| 1 | **Step-level plan for checks-only inputs**: a change to conformance code marks an app's checks, not its Rust tests and e2e | **22% of member branch job-min** (model over 114 real member diffs: 2,613 of 11,778 job-min; 23 members lose 10 job-min of e2e each, Rust halves 12% more) | 1 to 1.5 | Low: the steps that do not read conformance are skipped only when the planner proves it; a golden test pins both planners | Train round full (ER-8) |
| 2 | **WebKit at device scale 1 on Linux** (Debrief, Dyno, Pitwall, Spotter take `Desktop Safari`'s `deviceScaleFactor: 2`, so WebKit paints 4 times Chromium's pixels; `ui/` already uses 1) | Not measured. If Debrief WebKit falls to Dyno's ratio (1.3x Chromium): about 1,900 test-s, **about 16 job-min a train round**, plus the other suites | 0.5 (A/B, 20 runs per engine) | Retina rounding: half-pixel layout reads can differ. Decision D-2 keeps a scale-2 set if wanted | Full WebKit still every train |
| 3 | **E2e cases that duplicate component tests move down a level** (keep one smoke case per feature) | Debrief about 225 of 704 cases, **about 2,680 test-s a train round, 22 job-min**; Dyno about 75 cases, 220 test-s (est., sampled) | Debrief 3 to 5, Dyno 1 | Low with criteria (section 3); reachability stays through the smoke case | Component tests run on every branch |
| 4 | **Member runs use the nextest package filter that pull requests already get** (`narrow` in `matrix()` is `pull_request` only) | Small: nextest execution is 5 Linux job-min a full round; compile not measured | 0.25 | None: `rdeps` closure is the planner's own | Train round full |
| 5 | **Trains run a WebKit subset, full WebKit nightly and before every tag** | About 2,750 WebKit test-s, **about 23 job-min a train round** (the class-B/C files of section 3) | 0.5 | Medium: a WebKit-only defect in those files is seen up to a day later. Needs rule 3 changed and the nightly re-enabled (paused until Dyno 1.2.0) | Nightly and tag runs full; history in section 2.3 |
| 6 | Playwright `--only-changed` on branch rounds | 1 of 114 members, 0.2% | 0.25 | Unsafe for app source: the import graph does not see code the browser loads | Use it locally only |
| 7 | Coverage-based test selection (TIA) for e2e or Rust | Ceiling **1.9%** of member job-min (12 members whose e2e was reached by app source) | 3 to 5 | Misses what coverage does not see (CSS, timing, config) | Not recommended |
| 8 | Windows fixture repositories made once per test binary and copied | Not measured; paddock-dev on Windows is 4,334 test-s against 814 on Linux; a small fixture repository costs 238 to 323 ms there | 1 to 2 (ADR 0108 owns it) | None | Same tests |
| 9 | One app boot per worker (shared page across tests) | At most 13% of Debrief WebKit (app start 0.6 to 1 s a test) | 2 | High: breaks per-test isolation (rule 49) | Not recommended |
| 10 | More Playwright workers, nextest archives | None on a CPU-bound host (job-cost note 3.2, 4) | | | Not recommended |

**Top five, in order: 1, 2, 3, 4, 5.** Together, per member branch round about 22 job-min of 103 (mean, item 1; item 3 adds
about 8 when Debrief's UI is reached), and per train round about 40 job-min from items 2 and 3 (16 + 22 + Dyno's 4; item 5's 23
overlaps them, since its files are the ones item 3 moves), for 6.5 to 9 agent-days. Cheapest common-practice
option for each is in section 5. Decisions: section 6.

## 1. Test-level selection for branch rounds

### 1.1 What selects today (verified in code)

- **Project level.** `scripts/affected.mjs` (and `./dev plan`, held equal to it by paddock-dev's golden test) maps changed files
  to projects (`crates`, `ui`, `conformance`, `tools`, `paddock-dev`, `icons`, `eslint`, `app:<name>`) through the Cargo path
  graph, the npm graph, Tauri `frontendDist`, tsconfig/angular/ESLint paths and declared doc inputs (file header, lines 1 to 30).
  An app is marked with halves, `rust` and `ui` (`kind()`, line 639); a marked app with no half named gets both.
- **Crate level, pull requests only.** `matrix()` builds a nextest filter `package(=a) | package(=b)` from the plan's
  `packages` when `narrow` is set (line 877), and `ci()` sets `narrow: event === 'pull_request' && !env.PLAN_APP` (line 1186).
  Train, trial and member runs are `workflow_dispatch` (all 50 runs listed on 2026-11-20 were dispatch, push or schedule), so
  **no current run uses it**. The shared `crate` job runs the whole workspace; its `FILTER` is only the fix ladder's failed
  tests (`.gitea/workflows/ci.yml` lines 1038 to 1042).
- **Test level**: none on the first round; the fix ladder reruns failed tests by name.

### 1.2 Measured on real member diffs

Method: the 114 distinct member branches merged into trains 13 to 16 since 2026-11-14 (`git log origin/main --merges`, the
`chore(train): merge <branch> (<sha>) into train N` commits); each member's diff is its sha against the merge base with the
train it joined. The planner was run on each (`node scripts/affected.mjs <base> <sha>`, origin/main's graph). Each plan was
priced with the per-job seconds of full round 8335/8338 in branch mode (Chromium only, no bundles, no smoke tests; all OS),
using the plan's halves. Member diffs: median 9 files, p90 61.

| | Job-min (114 members) | Share |
|---|---:|---:|
| Today's plans, priced | 11,778 (mean 103, median 90 a member) | 100% |
| A1: an app's e2e marked though the change touches nothing the e2e reads | 1,187 (23 members, 60 job-min each at most) | 10.1% |
| A2: an app's Rust half (nextest, compile, Windows and macOS legs) marked the same way | 1,426 | 12.1% |
| B: a member whose app change is e2e specs only, run the changed specs only | 21 (1 member) | 0.2% |
| C: members whose e2e is reached by the app's own source (where coverage-based TIA could help) | 228 of e2e (12 members) | 1.9% |
| Nextest by changed crates' `rdeps` in the shared crates (execution only) | 57 | 0.5% |

**Why A1 and A2 happen** (verified): a change under `conformance/src/` or `conformance/bin/` marks every app whose checks run
paddock-conformance, with no half named, so both halves run on every OS (`scripts/affected.mjs` lines 679 to 682: "The apps
whose own checks run paddock-conformance (C13 and others, ADR 0060) rerun those checks when conformance changes"). Apps without
such a check are reached through the npm graph (the `--explain` reason `conformance <file>`). Example `feat/c30-shrink-only`
(7 files: C30's check, its allow list, a util, two tests, two docs): every app, `rust` and `ui`, priced at 155 job-min, of which
60 are e2e and 67 Rust legs that read none of those files. The checks step itself (seconds) is what the change needs.

"Reads nothing" was decided by a path rule (`bin/select2.py`): an app's UI inputs are `apps/<app>/app/`, `ui/`, the lockfile,
`tsconfig/`, `eslint/`, `icons/`, `.gitea/` and the root npm files; its Rust inputs are the app outside `app/src` and
`app/e2e`, `crates/`, `.cargo/`, `lint/`, `.gitea/` and the toolchain file. It is conservative: any `.gitea/` change counts as
read by both. It is an estimate of the planner change, not the planner itself.

**Why B and C are small**: members that change e2e specs nearly always change the app's source or a shared UI input too (22
members touch specs, 1 touches only specs). And only 12 of 114 members reached an app's e2e through that app's source alone;
the rest reached it through shared inputs (ui, the lockfile, workflows), which a per-test map would select almost entirely.

### 1.3 Finer selection, against common practice

- **Playwright `--only-changed`** (since 1.46): "will only run test files that have been changed since the last git commit
  ... This will also run all test files that import any changed files" (<https://playwright.dev/docs/release-notes>). The
  graph is the imports the test runner itself loaded (`collectAffectedTestFiles` over `fileDependencies`,
  <https://raw.githubusercontent.com/microsoft/playwright/main/packages/playwright/src/transform/compilationCache.ts>). The
  app under test is served by `webServer`, so a change to app source selects **no** spec: unsafe as a CI selector here (the
  reading of the source is ours; the docs do not state the limit). Shared fixtures that specs import are followed. Useful as a
  local inner loop only.
- **nextest filtersets**: `rdeps(x)` "Include all tests in crates matching name-matcher, and all the crates that (possibly
  transitively) depend on" it (<https://nexte.st/docs/filtersets/reference/>). The planner already computes that closure
  (`packages`). Note that `-E` filters tests after building the whole workspace; only `-p` narrows the build.
- **Target-graph selection**: Nx "selects a task when your change touches one of its inputs" and "`dependsOn` alone doesn't make
  a task affected" (<https://nx.dev/docs/features/ci-features/affected>); Bazel reruns a test only when "Bazel detects changes
  in the test or its dependencies" (<https://bazel.build/docs/user-manual>). This is item 1: declare what each step reads, as
  Nx does per task, instead of marking a whole app.
- **Coverage or history TIA**: Datadog TIA works by "analyzing your test suite to identify the code each test covers", runs
  everything on the default branch, supports no Rust and lists no Playwright
  (<https://docs.datadoghq.com/tests/test_impact_analysis/>,
  <https://docs.datadoghq.com/tests/test_impact_analysis/setup/javascript/>). Launchable / CloudBees Smart Tests learns from
  "the historical correlation between changed files and failed tests"
  (<https://help.launchableinc.com/features/predictive-test-selection/how-launchable-selects-tests/>); its 35 to 70% runtime
  claim is customer-reported (unverified). Meta's predictive selection halved test cost while "over 95% of individual test
  failures and over 99.9% of faulty changes" were still reported (<https://arxiv.org/abs/1810.05286>). Google TAP selects by
  reverse dependencies and runs the skipped affected targets together at milestones
  (<https://huang.isis.vanderbilt.edu/cs8395-f23/paper/google-testing-icse-seip-17.pdf>). All keep a full run behind the
  selection, as Paddock's train round is. At Paddock's scale the ceiling (1.9%) does not pay for 3 to 5 days, and no
  per-test coverage mode is documented for cargo-llvm-cov (<https://github.com/taiki-e/cargo-llvm-cov>).

**Safety net.** Every member is still tested in full on the train round (ER-8), and failures there pull the member (ER-8,
rule 50). The nightly that used to be the second net is **paused** until Dyno 1.2.0 (decision 2026-11-20, scheduled CI
paused), and D4 removes the main re-run, so the train round is today the only full run: nothing here may shrink it.

## 2. Debrief WebKit: why about 2x Chromium

### 2.1 Measured, per test

Three full Debrief rounds with both engines: 8338 (train 15, e0fd0f511), 8328 (train 15, 6199bc29b), 8316 (main push of train
14, 9cbd77a63); 692 test pairs each (same spec, line and title in both engines).

| Run | WebKit test-s | Chromium test-s | Ratio of sums | Median per-test ratio (p10 to p90) | Fit: WebKit = a + b x Chromium |
|---|---:|---:|---:|---|---|
| 8338 | 5,086 | 2,443 | 2.08 | 1.94 (1.25 to 3.05) | 0.32 s + 1.99 x |
| 8328 | 5,022 | 1,968 | 2.55 | 2.42 (1.61 to 3.54) | 0.54 s + 2.36 x |
| 8316 | 5,370 | 2,898 | 1.85 | 1.69 (1.01 to 2.75) | 0.00 s + 1.85 x |

**The extra cost is proportional to the work, not a start-up cost.** The intercepts are 0 to 0.5 s; the fastest tenth of the
tests (those that load the app and read little) cost about the same in both engines (0.43 to 1.0 s Chromium, 0.72 to 0.93 s
WebKit). The ratio moves with the run (1.85 to 2.55) because shards ran under different load: in 8328 the two WebKit shards ran
together after the Chromium ones, in 8316 all four overlapped. Other suites: Dyno 1.31x, Pitwall 1.69x, Spotter 1.36x (8335).

By file, the largest ratios are the live and moving views: `timeline` 3.9x, `ai-chat` 3.4x, `tools-monitor-charts` 3.2x,
`live-tail-follow` 3.0x (513 against 170 s), `popout` 2.8x, `live-tail` 2.8x, `dock-vs` 2.6x; static ones are low
(`dialog-window` 1.18x over 74 cases, `ux-import-progress` 1.6x, `themes` 1.6x).

### 2.2 The slowest 20 WebKit tests and their Chromium twins (mean of the runs that had them)

| Spec:line | WebKit s | Chromium s | x | Runs | Test |
|---|---:|---:|---:|---:|---|
| dock-panels:33 | 76.0 | 25.0 | 3.0 | 3 | no panel lets positioned content escape its scrolling lists |
| live-tail:121 | 34.7 | 9.2 | 3.8 | 1 | alerts: mute this kind for 15 min stops its toasts |
| activity-errors:31 | 34.1 | 16.0 | 2.1 | 3 | a view that throws is contained and logged |
| live-tail:139 | 30.6 | 7.7 | 4.0 | 2 | alerts: mute this kind for 15 min (line moved) |
| markers-highlight:26 | 29.9 | 17.6 | 1.7 | 3 | highlight lines with a regex and a badge, reorder, disable, delete |
| jobs:110 | 29.5 | 9.6 | 3.1 | 3 | notifications: a finished job is an unread message |
| live-tail:246 | 29.2 | 8.3 | 3.5 | 1 | rolling files of one log |
| live-tail-follow:255 (newest first) | 27.9 | 8.0 | 3.5 | 3 | a scrollbar drag across refresh rounds |
| live-tail:119 | 26.8 | 5.0 | 5.3 | 2 | an alert toast says its severity in words |
| live-tail:206 | 26.4 | 8.8 | 3.0 | 1 | the tail status panel |
| live-tail:145 | 25.4 | 6.5 | 3.9 | 1 | the alerts panel lists, filters, clears |
| activity:54 | 24.6 | 10.8 | 2.3 | 3 | a user journey is logged once per operation |
| live-tail-follow:255 (oldest first) | 24.5 | 9.3 | 2.6 | 3 | a scrollbar drag across refresh rounds |
| dock-vs:146 | 24.3 | 5.5 | 4.4 | 3 | auto hide button collapses a tool pane |
| live-tail-follow:316 (oldest first) | 22.4 | 9.4 | 2.4 | 3 | a wheel in the scaled scrollbar mode |
| tools-monitor-charts:22 | 22.3 | 8.4 | 2.7 | 3 | charts every metric per process |
| ux-import-progress:86 | 22.2 | 14.7 | 1.5 | 3 | the phases explain themselves |
| jobs:88 | 22.0 | 12.6 | 1.7 | 3 | a waiting AI request can be cancelled |
| timeline-header-geometry:35 | 21.1 | 7.8 | 2.7 | 3 | the zoom and menu buttons stay put at 1100 px |
| live-tail-follow:316 (newest first) | 21.0 | 7.9 | 2.6 | 3 | a wheel in the scaled scrollbar mode |

The top 20 tests are 10% of WebKit's seconds; the top 10 files 33%, the top 20 files 45% (146 files): the cost is spread, so
fixing a few specs does not remove it.

### 2.3 Causes, and what one trace run confirms

Already measured (lessons.md, "two live-tail e2e timed out in WebKit because every live tick costs the page about a second",
CI run 8243): in the CI image with WebKit, the page drew 3 to 18 frames in 3 s under the mock's ticks against 184 without them;
JS in timer callbacks was 12% of the time, forced layout 9 ms in 5 s, Intl 2 ms; the two tests took 32 to 37 s, Chromium 5 to 6
s, **macOS WebKit 7 s**. So the cost is the page's rendering after each update in **Linux headless WebKit**, not JS, waits or
app start.

New finding (verified in code): Debrief, Dyno, Pitwall and Spotter take `devices['Desktop Safari']`, which sets
`deviceScaleFactor: 2` (Playwright 1.64.1's `coreBundle.js`, "Desktop Safari": `deviceScaleFactor: 2`), and override only the
viewport (`apps/debrief/app/playwright.config.ts` line 51, 1440 x 900). Chromium's `Desktop Chrome` is scale 1. So Debrief's
WebKit paints 5.2 million device pixels a frame against Chromium's 1.3 million, in software. `ui/playwright.config.ts` line 28
already sets `deviceScaleFactor: 1` for its WebKit projects. No Debrief e2e reads `devicePixelRatio`; four Dyno specs do
(three are screenshot specs that CI skips, and `tray-widgets.spec.ts`).

Hypotheses, each with what refutes it:

1. **4x pixels in a software painter** (likely): refuted if Debrief WebKit at scale 1 is no faster than at 2.
2. **WebKit's per-update style and layout cost on Linux** (part of 1's slope): refuted if the trace shows WebKit and Chromium
   spend the same time per frame at equal pixels.
3. **Playwright's per-action protocol cost in WebKit**: refuted if the trace's action times are near Chromium's and the gap is
   in the waits for the page.
4. **Load on the shared host** (shown by the ratio's range): not a cause of the gap, a scale on it; refuted for the gap by
   running both engines alone.

**The one run that decides it** (0.5 day, in the CI image, a quiet host): the 20 tests above, WebKit at scale 2 and at scale 1,
and Chromium, `--trace on --repeat-each=5 --workers=2`; compare total time and, in the trace viewer's Actions tab, each action's
duration ("how long each one took to run", <https://playwright.dev/docs/trace-viewer>). Before a switch, ER-28's 20 runs per
engine.

### 2.4 What WebKit uniquely catches

From `docs/lessons.md` (77 WebKit mentions) and `git log origin/main -i --grep=webkit` (143 commits since 2026-09-01), the
product defects only WebKit showed:

| Class | Example (source) | Kind of spec |
|---|---|---|
| CSS WebKit prefix (`-webkit-user-select`) | lessons.md "WebKit (Safari, and WKWebView...)" (1); 0eb28cbf1, 45041266f | Now a static test, `scripts/webkit-prefix.test.mjs` |
| A click does not focus a button; Tab skips buttons | same lesson (2), (3); e275e1b6b (Pitwall, macOS WebKit) | Keyboard and focus specs |
| `0.01ms` transitions are real | same lesson (4); 153799f49, b661502f0 (axe scheme) | Theme and axe specs |
| Text metrics and widths | 47ed9b687 (Dyno: 782 against 758 px in WebKit), a67d2db84 (Explorer at 900 px), 3b3cdd509 | Layout and geometry specs |
| Scroll anchoring, clamped writes, dropped wheel input | 4c44f3561 (1 in 75 WebKit, 0 of 340 Chromium), the live-tail ADR 0072 rounds | `live-tail-follow` |
| Modifier key from the page's platform | c29508d9d (`ControlOrMeta`, WebKit on Linux) | Shortcut specs |

All of these sit in what section 3 calls class A (geometry, focus across regions, rendering, real input). The WebKit failures in
class B/C specs found in the same period were test races present in both engines, which WebKit's slower clicks widened
(lessons.md, "live-tail toast clicks": "the race is the same in both engines"; the `ai-chat` and `jobs` flakes of train 14).
That is the basis for item 5's subset: class-A files, plus one smoke case per feature, on every train; the rest nightly and
before each tag. **Rule 3 says the full suite, Chromium and WebKit, runs once on the combined result before a local build or
release, plus CI**; a train subset changes that rule, so it is the user's decision (D-4), and it needs the nightly back.

## 3. Duplicate coverage: e2e that could be component tests

Both apps already run component tests with Vitest through `@angular/build:unit-test` and jsdom (`apps/debrief/app/angular.json`
lines 113 to 120, `apps/dyno/app/angular.json` line 87): Debrief 472 spec files (332 with TestBed), Dyno 167 (84). Angular's own
guide now ships Vitest and jsdom by default (<https://angular.dev/guide/testing>). Neither app uses Testing Library.

**Criteria** (all must hold for a case to move down): (1) no geometry: no size, overflow, scroll, clipping, text fit (jsdom has
no layout); (2) one window: no popout, dialog window, reload or startup; (3) no real input or rendering: no drag, wheel, hover
tooltip, canvas, contrast or CSP; (4) keyboard only inside one component; (5) no full-page axe scan as the point; (6) the mock
backend with fixed inputs; (7) the assertions are about one component and its store, not two components agreeing on live data.
Class A fails one of 1 to 5; B meets all; C is mixed.

| App | Cases | A | B | C | Movable (est.) | Movable Chromium s | Movable WebKit s |
|---|---:|---:|---:|---:|---:|---:|---:|
| Debrief (149 files) | 704 | 360 | 108 | 236 | about 225 | about 913 | about 1,767 |
| Dyno (26 files run) | 442 | 302 | 10 | 130 | about 75 | about 83 | about 136 |

Checked line by line (13 files): Debrief `updates` 7 of 8 cases duplicated (`e2e/updates.e2e.ts:16` against
`src/app/updates/updates.spec.ts:195`), `plugin-packages` 2 of 2, `lifecycle-banners` 3 of 4, `service-health` 2 of 2,
`settings-storage` 4 of 4 (both assert "Freed 57 MB."), `profile` 2 of 5, `export` partial; Dyno `helper` 8 of 8, `quit-menu`
2 of 2, `disk-map` 6 of 6 movable, `flows` 7 of 8 sampled, `lists-map` 4 of 5, `tray-widgets` 2 of about 5. Verified
duplicates: about 21 Debrief and 29 Dyno cases; in the sample about 85% of movable cases already had a matching component test
(the sample leaned to high string overlap, so this is an upper bound). The class split is a reader's judgement over titles for
every file and code for about 15.

**Keep one smoke case per feature** in the browser: deleting an e2e that a component test duplicates also drops the proof that
the feature is reachable from the real shell. Common practice agrees on the direction: "Push your tests as far down the test
pyramid as you can" (<https://martinfowler.com/articles/practical-test-pyramid.html>); Google suggests "70% unit tests, 20%
integration tests, and 10% end-to-end tests" (<https://testing.googleblog.com/2015/04/just-say-no-to-more-end-to-end-tests.html>).
Debrief today is 704 e2e cases against about 4,800 unit cases.

## 4. Cheaper mechanics

| Mechanism | Source | Here | Verdict |
|---|---|---|---|
| Worker-scoped fixtures and `storageState`: setup "once per worker" (<https://playwright.dev/docs/test-fixtures>, <https://playwright.dev/docs/auth>) | No login to share; the app's boot is 0.55 to 0.99 s a test (job-cost note 3.2), 13% of Debrief WebKit | Sharing one page across tests breaks per-test isolation (rule 49) | Not recommended; Dyno page-layout's one load per page is the exception already decided |
| `test.describe.configure({ mode: 'parallel' })`, `fullyParallel` ("Tests are split at the individual test level, leading to more balanced shard execution", <https://playwright.dev/docs/test-sharding>) | Debrief is `fullyParallel: true` (config line 33); Dyno's WebKit shards in flight | Shortens the critical path, not CPU | Only for balance |
| Workers: default "50%" of logical cores (<https://playwright.dev/docs/test-cli>); "We recommend setting workers to "1" in CI" (<https://playwright.dev/docs/ci>) | The Linux host is CPU-bound at 6 lanes (job-cost note 1.3) | More workers do not cut CPU | Not recommended now |
| Blob reports and `merge-reports` (<https://playwright.dev/docs/test-sharding>) | One report per shard today | Reporting, not cost | Neutral |
| nextest archive, build once run many (<https://nexte.st/docs/ci-features/archiving/>) and `partition` (`hash:m/n` is stable, <https://nexte.st/docs/ci-features/partitioning/>) | Each workspace is tested in one job per OS | Nothing to share (job-cost note 4) | Only if Windows paddock-dev stays the critical path |
| `test-threads` (default "num-cpus", <https://nexte.st/docs/configuration/reference/>) | `profile.ci` sets `"num-cpus"` (`crates/.config/nextest.toml` line 40) | Covered by the Linux CPU share (in flight) | Already decided |
| Process per test: "quite slow on Windows ... can be slow on macOS if anti-malware protections are interfering" (<https://nexte.st/docs/design/why-process-per-test/>); launching a program is ">20x faster" on Linux than Windows (<https://www.bitsnbites.eu/benchmarking-os-primitives/>) | Windows git start 33 to 44 ms, a small fixture repository 238 to 323 ms (benchmarks.md, "The shared reader moved in", runs 8277 to 8281); paddock-dev 4,334 test-s Windows against 814 Linux | Fixture repositories built per test with many git starts | Item 8: build each fixture once per binary and copy it, under ADR 0108 |

## 5. Common practice and the cheaper option (ER-37)

- Item 1: Nx and Bazel select per task by declared inputs; the cheaper option is a one-line planner change, conformance code
  marks an app's `rust` half only (its checks run in the Linux Rust leg), 0.25 day, which keeps the Rust legs (12%) and drops
  only the e2e (10%). Recommended: the full `checks` half (1 to 1.5 days), since it is the same change done once.
- Item 2: Playwright's own WebKit device is a Retina Mac; the cheaper option is the A/B alone, then decide.
- Item 3: the pyramid is the common practice; the cheaper option is to delete only the verified duplicates (about 50 cases, 0.5
  day) and leave the rest.
- Item 5: GitLab and GitHub merge queues run the full pipeline per train; a subset per train goes below the rule, so the cheaper
  option is **no change** (keep full WebKit every train) and take items 2 and 3, which cut the same seconds without moving any
  test out of the train.

## 6. Decisions for the user

- **D-1. Step-level plan** (item 1): build the `checks` half (recommended, 1 to 1.5 days) or the one-line `rust`-only form (0.25
  day, saves 10% instead of 22%).
- **D-2. WebKit scale** (item 2): after the A/B, run Linux WebKit at scale 1 everywhere, or at 1 with the geometry specs (class
  A layout files) kept at 2. The macOS WebKit legs stay at the device's scale either way.
- **D-3. Duplicates** (item 3): move all movable cases with one smoke case per feature (4 to 6 days over Debrief and Dyno), or
  only delete the verified duplicates (0.5 day).
- **D-4. WebKit subset on trains** (item 5): keep the full WebKit leg on every train (rule 3 as it is; recommended while the
  nightly is paused), or change rule 3 so trains run the class-A WebKit set and the nightly and tag runs the full set (only after
  the nightly is back).
- **D-5. Member runs narrow** (item 4): use the existing nextest package filter on member dispatch runs (0.25 day), yes or no.

## 7. Work packages

| WP | What | Days | Depends on |
|---|---|---|---|
| WP-1 | Planner: a `checks` half; conformance code marks it; the app leg runs only its `checks` step for it; `./dev plan` the same, its golden test against the script; a failing test first (`feat/c30-shrink-only`'s diff marks no e2e) | 1 to 1.5 | D-1 |
| WP-2 | WebKit scale A/B and trace run (section 2.3), results in benchmarks.md; then the config change with 20 runs per engine | 0.5 + 0.25 | D-2 |
| WP-3 | Debrief class-B files to component tests (duplicates deleted, missing component tests written first), one smoke case per feature | 3 to 5 | D-3 |
| WP-4 | Dyno class-B and movable C cases, the same way | 1 | D-3 |
| WP-5 | `narrow` for member dispatch runs with a base; `-p` instead of `-E` where the filter is only packages | 0.25 | D-5 |
| WP-6 | WebKit subset list generated from a tag in the spec file (not a hand list, ER-40), the train/nightly split | 0.5 | D-4, nightly back |
| WP-7 | Windows fixture repositories once per binary | 1 to 2 | ADR 0108 |

## Not measured

- The cost of WebKit at scale 1 (WP-2 measures it); the share of hypothesis 1 against 2 and 3.
- The planner model in section 1.2 prices plans from one full round's job times; it does not include queueing, and the path
  rule for "reads nothing" is an estimate of WP-1, not WP-1.
- Compile time saved by narrowing to `-p` (item 4); only nextest execution was priced.
- How many member rounds and train rounds run a day; savings are per round.
- Whether the class-B/C split holds for the files read only by title (about 130 of 175).
- Per-job CPU (the host sampler is not landed).

## Heads re-read before the commit

See the commit: re-read 2026-11-20 just before committing, `origin/main` e0fd0f511991, `origin/docs/ci-job-cost-research`
44d30408bd10, `origin/docs/ci-speed-research` 93bbd80637b3, unchanged.
