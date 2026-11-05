# Debrief 1.2: the full release, everything discussed, fastest path (2026-11-05)

Status: plan, docs only. Nothing was built or run. User, 2026-11-05: "the plan should release all features and fixes that
were discussed", and Debrief 1.2 comes directly after Dyno 1.1.0 (`mem:order-2026-10-28`).

**Result.** About **299 agent-days of remaining work, about 359 with the plan's +20 % for review and CI**. That is more than
the "about 200" quoted so far. At about 5 agents it is **about 79 working days (16 weeks) after the Dyno tag**. With the tag on
2026-12-01 (an assumption) that means late March 2027, not counting holidays or CI queues.

How to read the evidence:

- A path is read at `origin/main` `0e667eca7`, unless a branch is named. "Chain" is `debrief/search-chain-rebase` `e6f0d1de2`.
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
| Signing and notarisation (`apps/debrief/docs/roadmap.md`) | Ad-hoc on macOS (10-30); Pitwall 1.1 is first signed (D13, 11-05) | Debrief 1.2 ships unsigned, with First launch text |
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
| Fuzz fix | `fix/debrief-fuzz-2026-11-05` `d1f67d311`, 0 behind, app only | Land (X11) |
| DuckDB 2 move, stable ids, search order: wp1, wp2, wp3 | Chain, 37 behind; `deny.toml` has no RUSTSEC ignores there. Review: CHANGES | **18 commits carry session trailers (ER-5; counted)**; 5 subjects over 100 chars; store_pane tests; C37 for `import_ready.rs` (X10) |
| ADR 0062 | On the chain, Accepted | Lands with it |
| N1, N2 | `debrief/1.2-lean`, 37 behind; approved with P2s | P2s |
| NR1, J1 | `feat/debrief-nr1-names-rebuild-job`, 1145 behind; 4 P1s | Rebase and P1s; `paddock-jobs` part waits |
| Autoload off, N6, P2 store loops | `wip/debrief-1.2-lean-2026-11-04`, not reviewed; also dirty in `wt/debrief-prep` | Finish, review |
| Search budget setting | `debrief/search-budget-setting`, 1078 behind; one P1 | Y4 |
| Names catalogue checkpoint | `fix/debrief-bench-query-rss`, 325 behind; P1: not measured | Measure |
| Open fixes | `fix/debrief-explorer-resize` (Explorer not resizable; 15 commits, 0 of 54 files on main), `-ci-flakes`, `-import-range-flake`, `-renderer-live-timeout`, `-ai-tests-load`, `test/debrief-linux-baselines` | X11 |
| Failing tests 0.2, 0.3 | `debrief/1-2-wave0` (one commit), plus uncommitted files in `wt/debrief-w0-tests` (10-28) | Salvage |
| D-0 | Uncommitted in `wt/debrief-d0-catalogue` (10-28) | Restart from it |
| Deterministic plain-language search | `debrief/nl-search`, local only, 1 commit (5,244 lines) plus 15 dirty files (10-28) | Y2 |
| Bundle P1 | **Active:** `fix/debrief-report-privacy`, dirty at 15:03 today | X1 |
| Integration | **Active, local, not pushed:** `debrief/1.2-int` `352b63440` already merges the fuzz fix, bench-rss, ci-flakes, lean and the chain, so it holds the 18 trailers too | X10 before any push |

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
| X3 | ADR 0052: adoption 1.25 P; larger tour profile (D6) 0.5 J; the pre-next-tag set moved from Spotter to Debrief (WP-S, WP-C1a, WP-B4a, WP-C2) 5 P | ADR 0052, on `docs/adr-wiring-outcomes-ui` | 4 | 6.75 |
| X4 | ADR 0054 A-1 for Debrief (it lists only three other apps; its gate binds each app's next tag after Dyno's) | ADR 0054, on `docs/adr-os-lifecycle-and-edges-amend1` | 4 | 2.5 P |
| X5 | Rule 71: a positive control for each D7 performance gate and VM row | `docs/rules.md:284` | R | 1 J |
| X6 | Logging: family ADR 2 P (F3); Debrief adoption 1.5 J | readiness §1 | 4 | 3.5 |
| X7 | Debrief's waiver sites: C36 (7 parser crates), C39 (4), C53 (10), C54 (1), by 2026-12-19. Coordinate with `fix/waiver-sweep-apps` (absent at this fetch) | `paddock.waivers.toml`; readiness "Deadline" (family 13-18, 6-9 without Debrief and Pitcrew) | 3 | 5 J |
| X8 | Copies with `delete_by = debrief-v1.2.0` that no package deletes. Driver: ADR 1 J, move to `paddock-chromium` 3 P (X14), rename 0.5 J, taking `browser/runtime.rs`. Rest 3 J (`prompted-tools`, `clipboard`, `icon-check-script`, `diagram-export-script`, `instance-lock`, `run-lifecycle`, second `loopback-http-client`) | `docs/known-copies.toml` (36 entries); `handoff:` §10; ADR 0051 U9 | 3, 4 | 7.5 |
| X9 | Release path: notes from CHANGELOG, not git-cliff; signing key whitespace (Debrief unchecked, `mem:release-checklist-lessons`); `logs` unanchored in `apps/debrief/.gitignore:18`; `DEBRIEF_TRANSITION_*` deleted; `ci-image` libduckdb bump | `handoff:` §6, §9; `debrief.yml:5` | R | 2 J |
| X10 | Clean the chain before it reaches main: copies without trailers, short subjects, the store_pane tests, C37 | review file; ER-5 | 1 | 2 J |
| X11 | Land the open fix branches of section 1.3 | `handoff:` WP11 | R | 3 P |
| Y1 | AI memory (ADR 0015 accepted): M1-M6, `paddock-memory`, Memory page, Debrief first | `docs/research/2026-10-ai-memory.md:180-185` | 2 | 10.5 P |
| Y2 | Semantic search: X55 `paddock-embed` 3 P; templates with BM25 fusion, hit@5 >= 0.85 on the 36 queries 3 J; constrained Pitwall rewrite 1.5 J; tuned-model and rerank measurements 1 J; reconcile `debrief/nl-search` 1 J | `mem:no-vector-search`; `shared-next.md:573-590` | 2 | 9.5 |
| Y3 | DuckLake trial 3 P (does not gate the tag, 10-31); Quack question 0.5 J; DuckDB 2 columnar append against Parquet fan-out 1 J | `mem:ducklake-trial`, `mem:debrief-1-2-cpu-and-jobs`, `mem:debrief-import-fastest` | 1 | 4.5 |
| Y4 | Search budget 1 P; index repair path 1 P (user, 10-31, `handoff:` §11); kept-profile quit 0.5 J; `install.rs` 15 s and `sessions.rs:324` waits 0.5 J | as named | 1, 3 | 3 |
| Y5 | Debrief tray popup on `paddock-widgets` (Pitcrew's P7 is 8) | `mem:decisions-2026-10-31-vm-gui-ci`; `tray.rs` has none | 4 | 5 J |
| Y6 | Ports-and-adapters audit and a CI architecture check | `mem:backlog-2026-10-27` | 5 | 3 J |
| Y7 | Screenshot determinism in `paddock-e2e`, Debrief first | `mem:backlog-2026-10-27` | 5 | 2 J |
| Y8 | Test architecture, Debrief first: test ids, page objects, pyramid, flake list | `mem:test-architecture-after-v1` | 5 | 4 J |
| Y9 | Remove images and videos from Explorer 1; Share parser 1.5 (no code found); unit-bearing values to numbers 1 (not confirmed in code); job service on the ADR 0020 executor 1.5; `rss_bytes` rename 0.5; `QUERY_BUDGET_MB` re-measure 0.5; five Windows clippy errors 0.5 (verify first) | `mem:backlog-2026-10-27`, `mem:backlog-2026-10-21` | 1-4 | 6.5 J |
| Y10 | Rule 47 in Debrief's own views (only 4 files import Material; `<details>` in 13) 6; UI review 2 (P1 5, P2 10, P3 4) and what is left of the UX audit 5; new screenshots and drawings with independent review 1.5 | `mem:prefers-angular`; `memory/briefs/ui-review-2-debrief.md`; `docs/ux-audit/README.md`; ER-23 | 5 | 12.5 J |
| Y11 | 10 M events under 0.5 s; sticky field promotion | roadmap "Store performance"; `mem:backlog-2026-10-21` | 1 | 3 J |
| Y12 | Workbench move: its plan says 1.2 "grows by about 15" (0a, baseline retake, Phase 1, 2b, 2c, Phase 3) | `2026-10-29-shared-workbench-move.md:307-310` | 4 | 15 P |

**Sum of missing: 116.25** (53.25 P, 63 J).

**Not counted, needs the user:** the roadmap's other open items are not in the notes the brief names. Sized by judgement at 47.5:
format gaps 4, PML 3, gcdump and counters 3, line heat 3, Linux and macOS collectors 5, WebView2 4, snapshot automation 1, vision in
agent mode 1.5, WCAG manual pass 2, engine i18n 5, batch queries 2, R0 check 1, R3 split 5, R5 items 8. Adding them makes 57 with
review and about 13 more working days.

The plan's own total also drops one row: **D-0's 2 days are not in its wave 3 sum**, and its 183 leaves out N0-N8 (`plan` §5 says they add to it).

## 2. Fastest path

### Totals

| | Agent-days | Basis |
|---|---|---|
| Plan packages, summed row by row | 195.25 | P: 182.75, + D-0 2, + N0-N8 10.5 |
| Already done or on a branch | -12.5 | J: 1.1 and 1.2 3.5; IX-ADR 1; IX-B1 3; N1, N2 1.5; NR1, J1 1.5; D-a 1; 0.5 0.5; P2, N6 0.5 |
| Missing packages | +116.25 | Section 1.5 |
| **Remaining** | **299** | |
| **With the plan's +20 %** | **about 359** | The plan's own rule; its rows are ±50 % |

Elapsed, by judgement:

- **Capacity:** 5 agents, 85 % busy (the plan's factor), is 4.25 agent-days per working day. Two other limits: Dyno's priority before the tag, and heavy jobs through slots at `CARGO_BUILD_JOBS=2`.
- **Before the tag:** about 1.5 agents for 18 working days (Dyno's estimate of 10 to 14 days with four agents, plus slack; `mem:order-2026-10-28` says early December) is about 23.
- **After the tag:** (359 - 23) / 4.25 = **about 79 working days, about 16 weeks**. Capacity sets this, not the critical path (`plan` §5: 35.5 days of wave heads; about 45 with these additions, J), so more agents would help but ER-24 caps them.
- Anything else on the machine comes out of this: Spotter's 11 to 13 days, if run alongside, add about 2.5 weeks (readiness). Each week the Dyno tag moves, this moves by about the same.

### Lanes

| Lane | Remaining | Order |
|---|---|---|
| 1. Engine | 55.25 | X10, chain lands; D-a; 0.3, P0, D-0; P2, P3; SR4, SR1, SR3, SR5; IX-B2, IX-C, IX-M, IX-S; 3.5; 4.3; P4; 5.1; 5.2; D-b; B1; Y3, Y11 |
| 2. Names and AI | 34 | N1, N2, N6, NR1; J1; N3, N4, N5, N0; X55; N7, semantic search; Y1; N8 |
| 3. Plugins and processes | 70 | X7; 0.4; 1.4; 2.2, P1, A1, BP1; IP1; 2.6a, 2.6b, 2.6c; PH1, PH2a; 2.6d; PH2b; PH3, PH4; 3.4, P5, 3.2, 4.4; LV1, LV2; X8 driver |
| 4. Shell | 66.75 | X1; 0.1, 0.2, 0.5; Y12 Phase 1; M1; M2a, M2c; 1.2's remainder, M2b; M3a; M3b; M4; X2, X3, X4, X6, Y5 |
| 5. Lists, limits, UI | 59 | 1.3; 2.1, 2.5; 2.7, 2.4, 3.3; 4.1, 3.7, 4.2, 4.5; 2.3, 3.6; Y6, Y7, Y8; Y10 |
| R. Release | 14 | X11 early; 5.4, X5, X9; M5, F, R, LV3, VM pass, tag |

Lane 3 needs slightly more than one agent; lane 2 needs less, so they share.

### Phases (J; T is the Dyno tag, working days)

| Phase | When | What runs |
|---|---|---|
| 0 | Now to T | Lanes 1 and 2, app parts; X1, X7, X11; designs; new crates on branches |
| 1 | T to T+10 | Rebase onto the tag; 0.1, 0.4, 0.5; 1.3, 1.4, 2.2, P1, A1, IP1, M1 |
| 2 | T+10 to T+30 | M2a, M2c, J1, M2b; 2.1, 2.5; 2.6a-c, PH1, PH2a; X2, X55, Y1 |
| 3 | T+30 to T+55 | M3a, M3b, M4; PH2b, PH3, PH4, 2.6d; 2.4, 2.7, 3.3; X3, X4, X6, X8 |
| 4 | T+55 to T+70 | 3.4, 4.4, LV1; 4.1, 3.7, 4.2, 4.5, 3.6; Y5, Y10; 4.3's budgets, B1 |
| 5 | T+70 to T+79 | 5.4, X5, X9, M5, F, R, LV3, VM pass, tag |

### When each package can start

| Class | Packages | Rule |
|---|---|---|
| **Now, app only; may land on main** | X1, X7, X10, X11; D-a's app part; 0.2, 0.3, P0, D-0, P2, P3; SR, IX; 3.5, 4.3, P4, 5.1, 5.2; Y3, Y11; search budget, index repair; N1, N2, N4-N6, N0, N8; NR1 without `paddock-jobs`; share parser, unit values. **About 70 raw (J)**, three times what the slots can do before T | Only `apps/debrief` and its docs. Landing one means a Debrief CI run; batch them (rule 50) |
| **Now, on a branch; lands after T** | New crates: `paddock-limits` core (1.4's first half), X55, `paddock-memory` (M1). Designs (no builds): 0.5, driver ADR, plan amendment. Y12's workbench-only paths (its plan allowed them; I keep them off main because Dyno builds that package) | User, 2026-11-05: shared code on branches is fine (`mem:free-slots-other-apps`). No shared-code landing before T (ER-1) |
| **Wait for T** | Everything else: anything that needs `paddock-setup` (absent from main), the shell and dialog code as Dyno leaves it, or that edits a crate Dyno changes (`paddock-proc`, `-jobs`, `-service`, `-updater`, `ui` shell). `ci-image` bump | `dyno/wp-int` is 350 commits ahead of main |

Benchmarks (IX-M, Y3, 4.3) need a quiet machine, so they wait for gaps even though they are app-only.

### Tag gates, and what the order change moves

Must hold on the tagged commit (`plan` §3-4 are unchanged):

- No `delete_by = debrief-v1.2.0` entry left. C58 does not enforce this on main yet, so it is by hand (`mem:free-slots-other-apps`).
- Debrief's waivers gone, DuckDB 2 in, and `deny.toml` without the ignores (`deny.toml:12-22`).
- ADR 0053 and the ADR 0052 and 0054 gates; the rule 54 dispatch; rule 71 positive controls.

Moving Debrief ahead also changes who merges what:

- **`paddock-jobs`:** readiness placed ADR 0052 WP-B1 (breaking) before J1. Now J1 lands first, because it is additive, and WP-B1 later updates Debrief too.
- **`paddock-assistant`:** Debrief's M3b now sets its shape before Pitwall's ADR 0023 work, so 0.5 must read ADR 0023.
- **Merge cost:** Spotter, Pitwall and Pitcrew branches now merge Debrief's shared changes rather than the reverse. This cost is not sized here.

## 3. Start now, in order

Shared terms for all of these: own worktree, `CARGO_TARGET_DIR` per app, `CARGO_BUILD_JOBS=2`, heavy lane `fix`, Dyno has priority, pause under load, affected tests only, no trailers.
Slots: code agents as load allows, design agents two (`mem:decisions-2026-11-05-readiness`, `mem:free-slots-other-apps`).

1. **X1, bundle P1** (1.5). Already running on `fix/debrief-report-privacy`; finish it. Owns `activity/report.rs`, `report/redaction.rs`. Failing test: a hostile log line and child log appear redacted in the zip; screenshots absent unless the checkbox is set. Dyno's `fix/dyno-bundle-p1` adds Redactor fixes: re-check after T.
2. **X10, a clean integration base** (2). Owner of `debrief/1.2-int`, before any push. Replace the 18 trailered wp2 commits with clean copies and shorten the 5 subjects. Fix store_pane's `opened()` fixture failures (build the sidecars first; if a binary is missing, the fixture names it) and add `ownTestBinaries` for `import_ready.rs`. Done when `git log origin/main..` has no trailer and the store-duck tests pass. Not a force push: the branch is unpushed.
3. **X11 first part, fuzz fix** (0.25). Land `fix/debrief-fuzz-2026-11-05` on main as a Debrief-only change. It is 0 behind; its two regression tests exist.
4. **X7, waiver sites** (5). Start by 2026-11-10, land by 2026-12-19. First check `fix/waiver-sweep-apps` to avoid doubling. Owns the 15 listed paths in `paddock.waivers.toml` (the integrator removes the rows) and C36's seven parser crates. Each site gets a failing check, then the fix; PH1 later deletes some, accepted by F1.
5. **Track A** (about 3). From `wip/debrief-1.2-lean-2026-11-04` and `wt/debrief-prep`: P2 (0 statements in 100 idle turns), autoload off, N6 (no request before the confirmation). Then NR1 without `register_keyed`, fixing the review's P1s (keyed specs through `submit_work`, `ai.button.busy`, fmt). J1's shared half goes on a branch.
6. **Failing tests and D-0** (6.5). Salvage `wt/debrief-w0-tests` and `wt/debrief-d0-catalogue` onto the clean base: 0.2, 0.3, P0, each failing for its stated reason, then D-0 until `debrief.yml:228`'s `continue-on-error` can go.
7. **Lane 1 engine** (SR1, SR3, SR5, then IX-B2; 10.75). Prototypes in parallel per ADR 0062 WP-1 and WP-8, on separate agents; IX-M only on a quiet machine.
8. **Design slot, no builds** (about 3.5). Land this plan into `plan`; 0.5 for section 1's left column, reading ADRs 0023, 0043, 0052, 0053, 0054; driver ADR (X8); M5 reworded for rule 46. Independent review of each.

## 4. For the user

1. **The size.** About 359 agent-days, not about 200. Agreeing to "everything discussed" is what adds the 116 missing days.
2. **Do the roadmap's other open items count as discussed?** Section 1.5, 47.5 days raw, about 13 working days. I recommend leaving them on the roadmap: nothing in the notes asks for them.
3. **Does ADR 0052's 5-day pre-tag set now gate Debrief instead of Spotter?** I counted yes, since Debrief is next.
