# Debrief 1.2: the full release, everything discussed, fastest path (2026-11-05)

Status: plan, docs only. Nothing was built or run. User, 2026-11-05: "the plan should release all features and fixes that
were discussed", and Debrief 1.2 comes directly after Dyno 1.1.0 (`mem:order-2026-10-28`).

**Result.** About **356 agent-days of remaining work, about 427 with the plan's +20 %**, not the "about 200" quoted so far. At about
5 agents, beside Spotter, it ends **about 103 working days after the Dyno tag: about 2027-04-01**, with the tag about 2026-11-07 and
work from Monday 2026-11-09. Holidays and CI queues are not counted. The work the user asked for most comes first: **Checkpoint A
about 2026-11-25 to 11-30, Checkpoint B about 2026-12-14** (section 2). They are progress points, not proposed cuts.

User decisions of 2026-11-05 recorded here (`mem:order-2026-10-28`, `mem:code-signing-later`):

- Within the full scope, import speed, idle CPU and jobs come first.
- Batch queries, format gaps and Process Monitor PML are in. The roadmap's other format and profiling items stay out.
- Scrutineer is dropped and all apps stay unsigned.
- Debrief runs side by side with Spotter (12 to 14 days).
- ADR 0052's pre-tag set is done by whichever app tags first, very likely Spotter.

How to read the evidence:

- A path was read at `origin/main` `0e667eca7` and re-checked where named at `87c9b8449`, unless a branch is named. "Chain" is `debrief/search-chain-rebase` `e6f0d1de2`.
- `plan` is `docs/plans/debrief-1-2.md`. Its SR and IX rows and its 183-day total exist only on the chain; section 8 (tracks) only
  on `debrief/1.2-lean` `fdf577eb7`.
- `mem:x` is a coordinator note (true when written, not checkable in git). `handoff:` is
  `/Volumes/d7sst7/dev-targets/handoff/debrief-1.2-plan.md`, outside the repo. `readiness` is
  `docs/plans/apps-readiness.md` on `docs/apps-readiness-plan`.
- Sizes: **P** comes from a plan or ADR, **J** is my judgement. Status "done" means the code or doc exists; I did not run it.
- Machine and agent logistics in the backlog notes (runners, disks, tokens) are not product items and are not listed.

## 1. Everything discussed

### 1.1 Decided against later: the later decision wins

| Earlier | Later | Result |
|---|---|---|
| Docked multi-webview browser (`mem:backlog-2026-10-21`; ADR 0024) and the user-driven panel (`mem:batch3-web-research`) | Chromium over DevTools, panel removed (`mem:backlog-2026-10-21`, 2026-10-22; ADR 0043) | Chromium, shipped |
| "Only the internal browser, no AI search" (batch 3, 2026-10-18) | Automatic Research job, on by default (`mem:batch4-ai-jobs`, same day, later entry; ADR 0060) | Research, shipped |
| No vector search for Debrief (`mem:no-vector-search`, 10-22, 10-23) | Semantic search over templates, "final" 10-25; N7 and `paddock-embed` in 1.2 (`mem:decisions-2026-10-31-remaining-apps`) | In 1.2. The plan's "N7 may slip to 1.3" loses. Y2 |
| "Starts after the Debrief release" (`mem:no-vector-search`) | "All features and fixes discussed" (2026-11-05) | In 1.2 |
| ADR 0015 M5: summaries on close at background priority | Rule 46: no LLM for background jobs (`plan` D9, a day later) | M5 stays, as a user-run action. Y1 |
| Background alias job; 1,500-entry names pack | Removed (D8); no curated pack (D10) | N1 on branch; N3 ships the mechanism only |
| DuckDB 2.0 after 2.0.1 (`mem:backlog-2026-10-27`) | Do not wait (D3) | 2.0.0 |
| Shared tracks wait for Dyno, Spotter and Pitwall (`plan` §8, on `debrief/1.2-lean`) | Debrief directly after Dyno (2026-11-05) | Wait for the Dyno tag only |
| Waivers fixed "inside 1.2" (`plan` §5) | All fixed by 2026-12-31, no extension (`mem:decisions-2026-11-05-readiness` F1) | Debrief's sites land before the tag. X7 |
| PH3 compatibility shim | Old plugins removed, no shim (`plan` §1, 2026-10-30) | PH3 as written |
| Signing and notarisation (`apps/debrief/docs/roadmap.md`); Pitwall first signed (D13, 11-05) | Scrutineer dropped, all apps unsigned (user, final, 2026-11-05, `mem:code-signing-later`; docs removed on main `87c9b8449`) | Debrief 1.2 ships unsigned, with First launch text |
| ICorProfiler attach mode (`mem:backlog-2026-10-20`, ~20:55) | Stopped by a safety check, events only (~21:10); roadmap "Dropped on purpose" | Stays out |
| Remote sources, WASM plugins, plugin catalogue | Dropped 2026-10-18 (roadmap) | Stays out |
| Offline mode | Cancelled 10-31 (`mem:decisions-2026-10-31-vm-gui-ci`) | Stays out |

Blocked, so it cannot ship in 1.2: **Code checks with Pacenotes** (BR1, roadmap "Next: Debrief 1.3") needs Pacenotes' bridge, and
Pacenotes follows Debrief. **macOS FFmpeg pin** waits for a trustworthy source (ADR 0033).

### 1.2 Done

Earlier backlog items, each shipped or covered by a named record on main:

| Item (source note) | Evidence |
|---|---|
| Lean Rust plugins, SDK, sandboxes, memory watcher (10-20, 10-21) | ADRs 0017, 0027; `debrief-plugin-*` |
| DuckDB store, Parquet segments, Tantivy, smart and substring search, projects (10-20, 10-21) | ADRs 0014, 0037; `smart_search.rs` |
| Jobs pane, Notifications, Research, explain as jobs (batch 4) | ADR 0012; ADR 0055 |
| Internal AI browser, agent mode, query draft and leak guard (batch 3, 10-21) | ADRs 0043, 0057, 0060; `browser/research.rs` `draft_search` |
| Dumps, UMDH, symbols by PDB identity, .NET and native profiling, `.etl` import (10-20) | ADRs 0019, 0022, 0023, 0036; roadmap "Profiling and dumps" |
| Mixed sources, parser library, AI parser generation, profile matching (10-20, 10-22) | ADRs 0020, 0026, 0032 |
| Notes, vision on one model, video and frames, Find related logs (10-20, 10-21, 10-22) | ADRs 0021, 0025, 0033; `debrief-video/src/analyze.rs` (mpdecimate) |
| Knowledge files, known issues, incident summary, playbooks (10-21, 10-22) | ADRs 0029, 0039 |
| Encodings, Paste as log, redacted copy, store housekeeping (10-22) | ADRs 0040, 0041; `CHANGELOG.md:111-112` |
| Spans by example, discovery, stacked tracks (10-20) | `features.md:29,36`; `debrief-core` span rules (grep only, flow not traced) |
| Themes incl. high contrast, density, native feel, perspectives, palette, minimap, history, search everything, shortcut editor, What's new, Problems pane (10-20, 10-21, 10-22) | ADRs 0018, 0028, 0035; `CHANGELOG.md:100,106`; roadmap "Workbench" |
| Dialogs are real windows (`mem:dialogs-are-windows`) | ADR 0050 |
| Performance settings, Suggest settings, Low memory (10-21, 10-22) | ADR 0031; `performance/suggest` |
| Native error snapshots (10-22) | ADR 0034; `window_capture/` |
| Runtime probing, real-world corpus, research live test, mock clock (10-21, 10-22) | `ai/core.rs`; `debrief-corpus.yml`; `browser/live.rs`; `core/mock/mock-clock.ts` |
| Shared dialog API (plan 1.1, 1.2) | `DialogFrame`, `eslint/src/rules/dialog-frame.cjs`, ADR 0043, `d636528da`. `expectDialogFits` is absent: 1 day stays |
| macOS footprint measure (start of plan 2.2) | `9d802de4c` |
| Zip duplicate paths (D13) | `debrief-core/src/archive.rs` |

### 1.3 On a branch

"Behind" is commits behind `origin/main`. All reviews are from `handoff/debrief-1.2-review-2026-10-31.md`.

| Item | Branch and state | Left |
|---|---|---|
| Fuzz fix | `fix/debrief-fuzz-2026-11-05` `d1f67d311`, 4 behind, app only | Land (X11a) |
| DuckDB 2 move, stable ids, search order: wp1, wp2, wp3 | Chain, 41 behind; `deny.toml` has no RUSTSEC ignores there. Review: CHANGES | 18 commits carry session trailers here (ER-5), but `debrief/1.2-int` is clean (below). 5 subjects over 100 chars; store_pane tests; C37 for `import_ready.rs` (X10) |
| ADR 0062 | On the chain, Accepted | Lands with it |
| N1, N2 | `debrief/1.2-lean`, 41 behind; approved with P2s | P2s |
| NR1, J1 | `feat/debrief-nr1-names-rebuild-job`, 1149 behind; 4 P1s | Rebase and P1s; `paddock-jobs` part waits |
| Autoload off, N6, P2 store loops | `wip/debrief-1.2-lean-2026-11-04`, not reviewed; also dirty in `wt/debrief-prep` | Finish, review |
| Search budget setting | `debrief/search-budget-setting`, 1082 behind; one P1 | Y4 |
| Names catalogue checkpoint | `fix/debrief-bench-query-rss`, 329 behind; P1: not measured | Measure |
| Open fixes | `fix/debrief-explorer-resize` (Explorer not resizable; 15 commits, 0 of 54 files on main), `-ci-flakes`, `-import-range-flake`, `-renderer-live-timeout`, `-ai-tests-load`, `test/debrief-linux-baselines` | X11a, X11b |
| Failing tests 0.2, 0.3 | `debrief/1-2-wave0` (one commit), plus uncommitted files in `wt/debrief-w0-tests` (10-28) | Salvage |
| D-0 | Uncommitted in `wt/debrief-d0-catalogue` (10-28) | Restart from it |
| Deterministic plain-language search | `debrief/nl-search`, local only, 1 commit (5,244 lines) plus 15 dirty files (10-28) | Y2 |
| Bundle P1 | **Active:** `fix/debrief-report-privacy`, dirty at 15:03 today | X1 |
| Integration | **Active, local, not pushed:** `debrief/1.2-int` `18cab509e`, 86 ahead and 4 behind main, merges the fuzz fix, bench-rss, ci-flakes, lean and the chain, with 0 trailers; 5 subjects still over 100 chars (counted) | X10 before any push |

### 1.4 In the plan, not started

All 65 packages of `plan` §2, plus SR and IX on the chain, not listed in section 1.3. That covers these items from the notes:

| Asked for | Packages |
|---|---|
| Idle CPU after import, name watcher, catalogue polling (`mem:debrief-1-2-cpu-and-jobs`) | P0-P5, NR1, BP1, B1, A1 |
| Job dedupe, busy buttons | J1 |
| Import as fast as possible; search ready soon after (`mem:debrief-import-fastest`) | SR1-SR5, IX, 4.3, 5.1, 5.2 |
| Plugin crash on Event Viewer logs, plugin memory, unlimited cap, process manager, lazy plugins, status-bar meters (`mem:backlog-2026-10-27` 6, 7) | PH1-PH4, 2.6a, 2.6b, 3.2, 3.4, 4.4 |
| Timeline choice, 10 lanes, virtualization (8) | 2.1, 2.4, 3.3, 4.1, 4.5 |
| 7.6 GB performance (9); streaming gzip, zip, CSV, JSON | 3.5, 4.3, 5.1, 5.2 |
| Dialog sizing (10); grid position after Explain and the colour-marked scrollbar (11) | M2b, 1.3, 2.1, 3.7 |
| Empty Video settings (2); image fit (3) | M2c, 2.3 |
| Limits explain themselves | 1.4, 2.3, 3.6 |
| Patience, not wall clocks | 2.6c, 2.6d, LV1-LV3 |
| DuckDB 2.0, Intel dropped | D-a, D-b, `plan` §4 |
| Quit ends a kept research profile (`plan` §7, no row, no days) | Y4 |

### 1.5 Missing from the plan: new packages

Lanes are in section 2. Failing test first for every fix (ER-11).

| # | Package | Source | Lane | Days |
|---|---|---|---|---|
| X1 | Bundle P1: logs and child logs through `paddock-diag`'s `Redactor` (`activity/report.rs:384-399`), screenshots off by default with a per-report checkbox | `mem:diagnostic-bundles-family` | 4 | 1.5 J |
| X2 | Per-machine install, guard, start check, notes (Debrief is `currentUser`) | ADR 0053 adoption table, on `docs/dyno-adr-0027-per-machine` | 4 | 3 P |
| X3 | ADR 0052 for Debrief: adoption 1.25 P; larger tour profile (D6) 0.5 J. The 5-day pre-tag set (WP-S, WP-C1a, WP-B4a, WP-C2) leaves Debrief: whichever app tags first does it, very likely Spotter. If Debrief tags first, those 5 days come back | ADR 0052, on `docs/adr-wiring-outcomes-ui`; user, 2026-11-05 | 4 | 1.75 |
| X4 | ADR 0054 A-1 for Debrief (it lists only three other apps; its gate binds each app's next tag after Dyno's) | ADR 0054, on `docs/adr-os-lifecycle-and-edges-amend1` | 4 | 2.5 P |
| X5 | Rule 71: a positive control for each D7 performance gate and VM row | `docs/rules.md:284` | R | 1 P |
| X6 | Logging: family ADR 2 P (F3); Debrief adoption 1 to 3 J (midpoint counted) | readiness §1 | 4 | 4 |
| X7 | Debrief's waiver sites: C36 (7 parser crates), C39 (4), C53 (10), C54 (1), by 2026-12-19. Already started: `wt/waiver-sweep`, branch `fix/waiver-sweep-apps`, local, 36 dirty files, no commits | `paddock.waivers.toml`; readiness "Deadline" (family 13-18, 6-9 without Debrief and Pitcrew) | 3 | 5 J |
| X8 | Copies with `delete_by = debrief-v1.2.0` that no package deletes. Driver: ADR 1 J, move to `paddock-chromium` 3 P (X14), rename 0.5 J, taking `browser/runtime.rs`. Rest 3 J (`prompted-tools`, `clipboard`, `icon-check-script`, `diagram-export-script`, `instance-lock`, `run-lifecycle`, second `loopback-http-client`) | `docs/known-copies.toml` (36 entries); `handoff:` §10; ADR 0051 U9 | 3, 4 | 7.5 |
| X9 | Release path: notes from CHANGELOG, not git-cliff; signing key whitespace (Debrief unchecked, `mem:release-checklist-lessons`); `logs` unanchored in `apps/debrief/.gitignore:18`; `DEBRIEF_TRANSITION_*` deleted; `ci-image` libduckdb bump | `handoff:` §6, §9; `debrief.yml:5` | R | 2 J |
| X10 | Clean the chain before it reaches main: copies without trailers, short subjects, the store_pane tests, C37 | review file; ER-5 | 1 | 2 J |
| X11a | Land the app-only fix branches: fuzz, `-ci-flakes`, `-ai-tests-load`, bench-rss with its measurement | `handoff:` WP11; tree diffs, 0 non-app files | R | 1 P |
| X11b | After T: `fix/debrief-explorer-resize`, `test/debrief-linux-baselines`, `fix/debrief-renderer-live-timeout`, `fix/debrief-import-range-flake` (9 to 10 non-app files each: workflows, shared actions, AGENTS.md) | same | R | 2 P |
| Y1 | AI memory (ADR 0015 accepted): M1, M2, M4, M5 as `paddock-memory` and the Memory page; **M3 and M6 go after M3b**; `paddock-assistant` stays on a branch until F6 | `docs/research/2026-10-ai-memory.md:180-185` | 2 | 10.5 P |
| Y2 | Semantic search: X55 `paddock-embed` 3 P; templates with BM25 fusion, hit@5 >= 0.85 on the 36 queries 3 J; constrained Pitwall rewrite 1.5 J; tuned-model and rerank measurements 1 J; reconcile `debrief/nl-search` 1 J | `mem:no-vector-search`; `shared-next.md:573-590` | 2 | 9.5 |
| Y3 | DuckLake trial 3 P (does not gate the tag, 10-31); Quack question 0.5 J; DuckDB 2 columnar append against Parquet fan-out 1 J; **hybrid fan-out then native** 1.5 J (still "pending" at `2026-10-29-duckdb-write-paths.md:249`) | `mem:ducklake-trial`, `mem:debrief-1-2-cpu-and-jobs`, `mem:debrief-import-fastest` | 1 | 6 |
| Y4 | Search budget 1 P; index repair path 1 P (user, 10-31, `handoff:` §11); kept-profile quit 0.5 J; `install.rs` 15 s and `sessions.rs:324` waits 0.5 J | as named | 1, 3 | 3 |
| Y5 | Debrief tray popup on `paddock-widgets` (Pitcrew's P7 is 8) | `mem:decisions-2026-10-31-vm-gui-ci`; `tray.rs` has none | 4 | 5 J |
| Y6 | Ports-and-adapters audit and a CI architecture check | `mem:backlog-2026-10-27` | 5 | 3 J |
| Y7 | Screenshot determinism in `paddock-e2e`, Debrief first | `mem:backlog-2026-10-27` | 5 | 2 J |
| Y8 | Test architecture, Debrief first: test ids, page objects, pyramid, flake list (166 e2e files, `plan` §3) | `mem:test-architecture-after-v1` | 5 | 8 J |
| Y9 | Remove images and videos from Explorer 1; Share parser 1.5 (no code found); unit-bearing values to numbers 1 (not confirmed in code); **job service onto the ADR 0020 executor 8: `Timetable` and `ManualExecutor` exist neither on main nor on `dyno/wp-int`, so it means WP-1 to WP-3 plus the `jobs/service.rs` part of WP-8 (1,475 lines)**; `rss_bytes` rename 0.5; `QUERY_BUDGET_MB` re-measure 0.5; five Windows clippy errors 0.5 (verify first) | `mem:backlog-2026-10-27`, `mem:backlog-2026-10-21`; `docs/adr/0020-shared-executor.md` | 1-4 | 13 J |
| Y10 | Rule 47 in Debrief's own views 12 (audit: 72 `<select>`, 63 checkboxes, 19 radios, 22 hand-rolled tabs; WP-DB is L, 2 to 3 sub-packages); UI review 2 (P1 5, P2 10, P3 4) and what is left of the UX audit 5; new screenshots and drawings with independent review 1.5 | `2026-10-28-material-cdk-audit.md:85,157` on `docs/research-material-cdk`; `memory/briefs/ui-review-2-debrief.md`; `docs/ux-audit/README.md`; ER-23 | 5 | 18.5 J |
| Y11 | 10 M events under 0.5 s; sticky field promotion | roadmap "Store performance"; `mem:backlog-2026-10-21` | 1 | 6 J |
| Y12 | Workbench move: its plan says 1.2 "grows by about 15" (0a, baseline retake, Phase 1, 2b, 2c, Phase 3) | `2026-10-29-shared-workbench-move.md:307-310` | 4 | 15 P |
| Y13 | User, 2026-11-05: format gaps 4, Process Monitor PML 3, batch queries 2 | roadmap | 2, 5 | 9 J |
| Y14 | Open from the 2026-10 redesign, in the notes: R0 check 1, R3 split 5, R5 UI items 8 | `mem:backlog-2026-10-20`, `mem:backlog-2026-10-21`; roadmap | 2, 5 | 14 J |
| Y15 | Overview freeze, measure and fix 2 (`mem:backlog-2026-10-21`; no record in `performance.md`); knowledge editor's unsaved-edit warning 0.5 (unconfirmed: no match in `knowledge-editor.ts`) | as named | 1, 2 | 2.5 J |
| Y16 | `paddock-about` and `paddock-deps`: AB1, AB2, DP1-DP4, IN1, IN2 (Pacenotes' 7.5 not counted) | `docs/adr/0028-about-and-deps-crates.md:482-` ("15 for releases"); `mem:post-release-about-deps-crates` | 4 | 15 P |

**Sum of missing: 173.25** (63.25 P, 110 J).

Staying out, as decided (user, 2026-11-05): the roadmap's other format and profiling items (gcdump and counters, line heat, Linux and
macOS collectors, WebView2, snapshot automation, vision in agent mode), the WCAG manual pass and engine i18n.

The plan's own total also drops one row: **D-0's 2 days are not in its wave 3 sum**, and its 183 leaves out N0-N8 (`plan` §5 says they add to it).

## 2. Fastest path

### Totals

| | Agent-days | Basis |
|---|---|---|
| Plan packages, summed row by row | 195.25 | P: 182.75, + D-0 2, + N0-N8 10.5 |
| Already done or on a branch | -12.5 | J: 1.1 and 1.2 3.5; IX-ADR 1; IX-B1 3; N1, N2 1.5; NR1, J1 1.5; D-a 1; 0.5 0.5; P2, N6 0.5 |
| Missing packages | +173.25 | Section 1.5 |
| **Remaining** | **356** | |
| **With the plan's +20 %** | **about 427** | The plan's own rule; its rows are ±50 % |

Elapsed, by judgement:

- **Capacity:** 5 agents, 85 % busy (the plan's factor), is 4.25 agent-days per working day.
- **Before the tag:** the tag is about 2026-11-07 (coordinator; `dyno-v1.1.0` does not exist yet) and work counts from Monday 2026-11-09, so the credit is about 2.5.
- **After the tag:** (427 - 2.5) / 4.25 = about 100 working days. Spotter's 12 to 14 days, side by side, take about 3 working days of capacity: **about 103 working days, about 2027-04-01**.
- **What limits it:** capacity, not the critical path; about 40 days are float. ER-24 caps the agents.
- **If the tag moves:** one week later moves the end by about 3.5 working days, because about 1.5 agents still work during that week.

### Checkpoints: what comes first

Not proposed as cuts. The order makes these exist early, whatever else happens.

| Checkpoint | When | Contains | Raw |
|---|---|---|---|
| **A** | About T+12 to T+15: 2026-11-25 to 11-30 | **Idle CPU:** P0, P2, P3, 2.2, P1 (H1). **Name learning:** N1, N2, NR1, N6, autoload off. **Jobs:** J1. **Import and search:** X10, chain, D-a, SR1, SR3-SR5, IX-B2, IX-M. **Obligations:** X1, X7, X9, reviews, VM pass | About 46 |
| **B** | About T+25: 2026-12-14 | A, plus the Event Viewer plugin fix (the chain to PH4) and dialog sizing (M1, M2a, M2c, M2b) | |

- **A's import figures:** IX-M at A measures serial, A and B. Its C row needs IX-C, which needs P4 and 4.3, so it comes later.
- **What would stop a tag at A or B:** `delete_by` binds `debrief-v1.2.0` (36 entries); the ADR 0054 and 0053 gates bind the next tag; `deny.toml:12-22` needs DuckDB 2 for 1.2.0. A release at a checkpoint would need another version number and a ruling on those gates. Nothing here assumes one.
- **Timing:** B's plugin chain is about 16.5 serial days (0.5, 1.4, 2.6a, PH2b, PH4), so one agent must start it by about T+8. That is why A is quoted up to T+15.

### Lanes

| Lane | Remaining | Order |
|---|---|---|
| 1. Engine | 61.75 | X10, chain lands; D-a; 0.3, P0, D-0; P2, P3; SR4, SR1, SR3, SR5; IX-B2, IX-M; 2.5's types with lane 5, then 3.5; 4.3; P4; IX-C, IX-S (needs BP1); 5.1; 5.2; D-b; B1; Y3, Y11, overview freeze |
| 2. Names, AI, sources | 46.5 | N1, N2, N6, NR1 (needs P3 and P0's `merge_scaling`); J1; N3, N4, N5, N0; X55; N7, semantic search; Y1 (M3, M6 after M3b); format gaps, PML; R3; N8 |
| 3. Plugins and processes | 70 | X7; 0.4; 2.2, P1 first; 1.4; A1, BP1; IP1; 2.6a, 2.6b, 2.6c; PH1, PH2a; PH2b; PH4, PH3; 2.6d; 3.4, P5, 3.2, 4.4; LV1, LV2; X8 driver |
| 4. Shell | 83.75 | X1; 0.1, 0.5; M1; M2a, M2c; 1.2's remainder, M2b; job executor; Y12; M3a; M3b; M4; X2, X3, X4, X6, Y5, Y16 |
| 5. Lists, limits, UI | 80 | 0.2; 2.5; 1.3; 2.1; 2.7, 2.4, 3.3; 4.1, 3.7, 4.2, 4.5; 2.3, 3.6; batch queries; R0, R5; Y6, Y7, Y8; Y10 |
| R. Release | 14 | X11a, X11b early; 5.4, X5, X9; M5, F, R, LV3, VM pass, tag |

### Phases (J; T is the Dyno tag, working days)

| Phase | When | What runs |
|---|---|---|
| 0 | Now to T | Section 3's first steps; 2.5 agent-days |
| 1 | T to T+15 | Checkpoint A; from T+8, the plugin chain |
| 2 | T+15 to T+25 | Checkpoint B: 0.4, 0.5, 1.4, IP1, 2.6a, 2.6b, PH1, PH2a, PH2b, PH4; M1, M2a, M2c, M2b |
| 3 | T+25 to T+55 | **Rest of import:** 2.5, 3.5, 4.3, P4, IX-C, IX-S, 5.1, 5.2. **Then:** BP1, 2.6c, 2.6d, 3.4, 4.4; M3a, M3b, M4, Y12; 1.3, 2.1 |
| 4 | T+55 to T+90 | 2.4, 2.7, 3.3, 4.1, 3.7, 4.2, 4.5, 3.6; X55, Y1; X2, X3, X4, X6, X8, Y5, Y16; R3, R5; Y10 |
| 5 | T+90 to T+103 | 5.4, X5, M5, F, R, LV3, VM pass, tag |

### When each package can start

| Class | Packages | Rule |
|---|---|---|
| **Now, app only** | X1, X7, X10, X11a; D-a's app part; 0.2, 0.3, D-0, P2, P3; P0 without `crates/paddock-proc/tests/`; SR1, SR3-SR5, IX-B1's rest, IX-B2; N1, N2, N5, N6; NR1 without `paddock-jobs`; search budget, kept-profile quit, the two waits; index repair after IX-B2; Y11; overview freeze; share parser, unit values; format gaps, PML, batch queries; R0; small Y9 items. **About 60 raw (J)**, far more than the 2.5 agent-days before T | Only `apps/debrief` and its docs. **Whether any of it may land on main before T is a coordinator decision.** Until then it stays on branches and `debrief/1.2-int` |
| **Now, on a branch; lands after T** | New crates: `paddock-limits` core, X55, `paddock-memory` (M1). J1 and the executor in `paddock-jobs` (not changed on `dyno/wp-int`). N3 (`paddock-packs`). Designs, no builds: 0.5, driver ADR, plan amendment | User, 2026-11-05: shared code on branches is fine (`mem:free-slots-other-apps`). No shared-code landing before T (ER-1). `paddock-assistant` stays on a branch until F6 |
| **Wait for T** | X11b; 2.5 and everything behind it (3.5 also owns `crates/paddock-archive`; then 4.3, P4, 5.1, 5.2, IX-C); IX-S (needs BP1, then P1 and 2.2 in `paddock-proc`); P0's `paddock-proc` half; N4, N0 (need N3); N8 (edits rule 46 and lessons); anything editing a crate Dyno changes: `paddock-proc`, `-service`, `-setup`, `-updater`, `-diag`, `-dialogs`, `-ipc`, `-window`, `-widgets`, `-download`, `-shell`, `-startup`, `ui`; `ci-image` bump | `dyno/wp-int` is ahead of main in those paths (tree diff) |

Benchmarks (IX-M, Y3, Y11, 4.3) need a quiet machine, so they also wait for gaps.

### Tag gates, and what the order change moves

Must hold on the tagged commit (`plan` §3-4 are unchanged):

- No `delete_by = debrief-v1.2.0` entry left. C58 does not enforce this on main yet, so it is by hand (`mem:free-slots-other-apps`).
- Debrief's waivers gone, DuckDB 2 in, and `deny.toml` without the ignores (`deny.toml:12-22`).
- ADR 0053 and the ADR 0052 and 0054 gates; the rule 54 dispatch; rule 71 positive controls.

Moving Debrief ahead also changes who merges what:

- **`paddock-jobs`:** readiness placed ADR 0052 WP-B1 (breaking) before J1. Now J1 lands first, because it is additive, and WP-B1 later updates Debrief too.
- **`paddock-assistant`:** Debrief's M3b now sets its shape before Pitwall's ADR 0023 work, so 0.5 must read ADR 0023.
- **Merge cost:** Pitwall and Pitcrew branches now merge Debrief's shared changes rather than the reverse. Spotter runs alongside, so it and Debrief must agree the landing order. This cost is not sized here.

## 3. Start now, in order

Shared terms for all of these: own worktree, `CARGO_TARGET_DIR` per app, `CARGO_BUILD_JOBS=2`, heavy lane `fix`, Dyno has priority until T, pause under load, affected tests only, no trailers.
Nothing lands on main without the coordinator's decision (section 2).

1. **X1, bundle P1** (1.5). Already running on `fix/debrief-report-privacy`; finish it. Owns `activity/report.rs`, `report/redaction.rs`. Failing test: a hostile log line and child log appear redacted in the zip; screenshots absent unless the checkbox is set. Re-check after T: `paddock-diag` changes on `dyno/wp-int`.
2. **X10, finish the integration base** (about 1 left). Owner of `debrief/1.2-int` `18cab509e`: trailers are gone. Shorten the 5 long subjects; fix store_pane's `opened()` failures (build the sidecars first; if a binary is missing, the fixture names it); add `ownTestBinaries` for `import_ready.rs`. D-a's app part follows. Done when the store-duck tests pass.
3. **X11a, app-only fixes** (1). Bring the fuzz fix, `-ci-flakes` and `-ai-tests-load` onto the base, and bench-rss with its A/B measurement (the review's P1; rule 48). Leave the four shared-path branches for T.
4. **P0 and the failing tests** (about 3). App half of P0, including `merge_scaling` (needed before NR1, ER-11), plus 0.3; salvage `wt/debrief-w0-tests`. Each fails today for its measured reason. The `paddock-proc` half and 0.2's shell parts follow at T.
5. **Idle CPU: P2, D-0, P3** (about 4.5, in that order). P2 from `wip/debrief-1.2-lean-2026-11-04` and `wt/debrief-prep`: 0 statements in 100 idle turns. D-0 from `wt/debrief-d0-catalogue`; removing `debrief.yml:228`'s `continue-on-error` is one line at T. P3 needs both: H3's test passes and status wakes on commit.
6. **Name learning: N1, N2, N6, autoload off, then NR1** (about 3). Close the P2s; N6 sends no request before the confirmation. NR1 only after P3 and a red `merge_scaling`: no `register_keyed`, keyed specs through `submit_work`, `ai.button.busy`, fmt. J1 (1.5) goes on its own `paddock-jobs` branch, landing at T.
7. **Import and search** (SR4, SR1, SR5, SR3, then IX-B1's rest and IX-B2; about 11.75). Separate agents per ADR 0062, on the clean base. SR3 needs P3. IX-M only on a quiet machine, after T.
8. **X7, waiver sites** (5). Already started in `wt/waiver-sweep`, 36 dirty files, no commits: its agent keeps it, and no one doubles it. Start by 2026-11-10, land by 2026-12-19. Each site gets a failing check, then the fix; the integrator removes the rows.
9. **Design slot, no builds** (about 3.5). Land this plan into `plan`; 0.5 for section 1's left column, reading ADRs 0023, 0043, 0052, 0053, 0054; driver ADR (X8); M5 reworded for rule 46. Independent review of each.

## 4. For the user

The size: about 427 agent-days and about 2027-04-01. "Everything discussed" is what adds the 173 missing days. Checkpoints A and B are where the most-asked-for work is in hand. Whether to release at one is the user's choice; it would need the ruling in section 2.
