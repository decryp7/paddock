# Debrief: foundation first, then usefulness (roadmap proposal, 2026-11-07)

A proposal, not a plan change: `docs/plans/debrief-1-2.md` is not edited. It builds on the usefulness audit
[2026-11-07-analysis-usefulness.md](2026-11-07-analysis-usefulness.md) (rows A1–A27, B1–B27 cited below) and on the plan
refresh, section 9 of `docs/plans/debrief-1-2.md` on `docs/debrief-1-2-plan-refresh` `abac43c0c` ("§9" below). The
audit's base `origin/debrief/1.2-int` `852b35ac0` has no section 9; its plan file ends at section 8.

Revision 2 (2026-11-07) answers the independent review of `cee49f7a1`: layer F's state, the `u32` ids, layer B's
existing columns, `ProcessInstance`, R1's conditions and the chain arithmetic, the stop-gap table, the totals.

- Marks as in the audit: **[S]** source at `origin/debrief/1.2-int` `852b35ac0`, **[P]** a size from the plan or an ADR,
  **[J]** judgement, **[calc]** worked out here, **[U]** not verified. Raw agent-days, ±50 %. Nothing was built or run.
- The user's priorities, 2026-11-07: "streaming performance and able to open folder is the important first step", and
  "build the best foundation for debrief". So the order here is: the streaming and folder-open chain, then the
  foundation layers the later features stand on, then usefulness work. Tiny items that do not compete for the same
  people may run beside the chain.

## 1. Foundation

Each layer: its state, the gaps that would force rework later for citations, the thread/sequence view, dump and UMDH
analysis, reports and Pitwall, the target design (existing ADRs, or the gap where none covers it), and the budgets it
must meet.

### A. Streaming import and back-pressure

- **State.** ADR 0051 (discovery and import as a bounded producer–consumer pipeline, 8,192-path queue, `2 × threads`
  files in flight) is built. ADR 0064 (Accepted 2026-11-06) moves every importer to one streaming trait; SI-0 to SI-18
  are planned (§9.2 layers 2–3), CSV streaming is on `debrief/stream-text` (not landed).
- **Gaps for later features.** Documents (dumps, UMDH, captures) are still read whole: the UMDH reader reads the file
  at once (`crates/debrief-plugin-dumps/src/umdh_reader.rs:78`) and the app process reads UMDH for its summary
  (`app/src-tauri/src/dumps.rs:557`); ADR 0064 row 6 and SI-11 cover it. UMDH and dump analysis must sit on SI-11's
  line loop and `Seekable` source, not on today's readers.
- **Target.** ADR 0064 as accepted.
- **Budgets.** No importer holds a whole file (ADR 0064); plugin memory cap 4 GiB (kept by the user with ADR 0067); the 1.43 GiB
  event log opens (§9.3 checkpoint, about T+19).

### B. Store and formats

- **State.** DuckDB 2 with Parquet segments and Tantivy; event ids are `u64` blocks per segment
  (`crates/debrief-store-duck/src/segments.rs:3`), never reused (asserted by the writer's test at `writer.rs:755`);
  every event already has `source` and `line` columns: the inspector's context reads by `(source, line)`
  (`crates/debrief-store-duck/src/context.rs:136`) and evidence ids are ordered by `ts, source, line, id`
  (`anomalies.rs:343`). ADR 0062 builds index and catalogue during the import; ST-1 to ST-6 and Q-1 to Q-4 are planned
  (§9.2 layer 2).
- **Gaps.** The thread/sequence view needs a total order inside a source and typed thread, process and activity
  columns. `(source, line)` gives it for text sources; what is missing is (a) a declared ordinal for sources without
  lines (an EVTX record id, a CSV row, an OTLP record index, a video frame) and (b) a guarantee that `(source, ordinal)`
  is unique and survives reload and re-time. Exact positions (`ord`) are decided only below 100M (§9.6 D7). Field
  values are not in the full-text index (`fts.rs`) [S].
- **Target.** ST and IX as planned, **plus** a small ADR 0062 amendment: `line` is the ordinal of text sources, each
  non-text importer declares its ordinal, uniqueness is checked at the sink (about 0.5 day of design, 1 of build) [J].
- **Budgets.** 1 billion events (ST-5, SI-15); post-import store CPU 20 CPU-s per GB against 57 measured (§9.6 D1).

### C. Plugin host

- **State.** The chain 0.5 → 1.4 → 2.6a/2.6b → 2.6c/2.6d → PH1/PH2a/PH2b → PH4 → 3.4 + SI-10 (§9.2 layer 3), IP1
  hardening; all wait for the Dyno tag (not tagged on 2026-11-07 [S: no `dyno-v1.1*` tag on origin]).
- **Gaps.** Plugins contribute importers, annotators and markers; there is no declared "analysis" contribution returning
  Findings (audit A27). Not needed in 1.2; design it against layer I's Finding type so it does not invent another.
- **Target.** Family ADRs 0001 and 0003, Debrief ADR 0017, ADR 0064 SI-7 to SI-10.
- **Budgets.** Stall judgement, not wall clocks (family ADR 0031); per-plugin memory cap; protocol 2.

### D. Folder open and selection

- **State.** ADR 0067 (Accepted 2026-11-06): quick scan, one judge (`ingest::judge`), tree grid, selection by folder;
  F1 to F12, FM memory gate, FR (39.75 days by its rows [P]). Today a folder imports everything with no choice (audit B5).
- **Gaps.** The folder survey (audit A14) samples after the import; it could run in F4's scan so the dialog proposes
  the instance key and parsers before anything is imported (1.3, 2 days) [J].
- **Target.** ADR 0067.
- **Budgets.** ADR 0067's (scan time, 14,000 files, the FM gate); ADR 0065's UI at about 15,000 rows.

### E. Evidence references, `u64` ids and process instances — no ADR yet

- **`u64` ids on every DTO and wire** [S]. The store's `EventId` is `u64` (`crates/debrief-batch/src/ids.rs:9`), but most
  DTOs carry `u32`: QL results (`crates/debrief-core/src/ql/mod.rs:59`), anomaly and outlier `sample_ids`
  (`analysis/anomaly.rs:85`, `analysis/outliers.rs:72`), Moment's `record_id`, the store's anomaly ids
  (`crates/debrief-store-duck/src/anomalies.rs:122`), report blocks and AI citations; `sample_id()` saturates to
  `u32::MAX` "until 6c widens them" (`anomaly.rs:89`; ADR 0014 promised the widening, it was not done). Ids are never
  reused, so a 1B store with re-imports gets within a few times of `u32::MAX` [calc: 4.29B]. A saturated id points at the
  wrong event silently. **Target**: `EventId` (u64) on every DTO, the generated TypeScript and the wire, decided together
  with ADR 0063 (the wire's integer encoding: JSON numbers lose precision above 2^53, so the TS side needs a string, a
  bigint, or a **declared bound**: ids below 2^53, checked at the sink with a typed error, so they stay plain numbers;
  2^53 is about 9 × 10^15, far above any store [calc]) and done **before** citations and report blocks. Size 3 [J].
- **`ProcessInstance`** [J]. Dumps (ADR 0068 §1: pid, start time, main module identity; rule 53), UMDH series (ADR 0069
  D1), the thread view (a thread id is only meaningful within one instance) and evidence references all need "the same
  process". One type in `debrief-core`: pid, start time (with ADR 0066 provenance), main image identity where known, and
  a confirmed/unconfirmed state with the reason. Size 0.5 after netting: the type and UMDH on it; the dumps' same-process
  check is ADR 0068 WP-6 [J].
- **State of event references** [S]. Three types point at an event: AI `Citation::Record { id: u32, verified }`
  (`crates/debrief-ai/src/assistant/citations.rs:11`), report `Block::Events { ids: Vec<u32> }`
  (`crates/debrief-core/src/report/mod.rs:108`), note anchors `Event { event_id: u64, time, source, excerpt }`
  (`crates/debrief-notes/src/anchor.rs`). `EventsRequest` leaves out ids that no longer exist
  without saying so (`crates/debrief-core/src/ports/dto.rs:87`). Whether a reload or a re-time keeps event ids is not
  traced [U]; ADR 0051 keeps **source** ids stable, not event ids.
- **Gaps.** The approved citations and report blocks, Pitwall drafting and every finding's evidence need one reference
  that survives reload, re-time (ADR 0066's Undo), a redacted copy and a project moved without its logs, and says
  "moved" or "gone" instead of vanishing. Building citations on the `u32` ids means undoing them later.
- **Target (gap ADR, proposed).** One `EvidenceRef` in `debrief-core`: stable source id (ADR 0051), a locator (the
  `(source, line)` or declared ordinal of layer B, byte offset for text, document key for a dump, UMDH call site or capture node), a
  content hash, the time with its provenance, and an id hint for the fast path. Resolution answers *found*, *moved*
  (resolved by locator and hash) or *gone* (with the kept excerpt), in ADR 0065's `Bounded` style. Notes, report blocks,
  AI citations and findings all use it (rule 55: one mechanism, three copies today).
- **Budgets.** Resolution of a 1,000-reference report under 200 ms on the 1B store [J target].
- **Size.** ADR 2, build 3 (types, resolver, the three users moved) [J].

### F. Analyses at scale — no ADR yet

- **State** [S]. On the DuckDB store (the default, ADR 0010) two analyses already cover every row: anomalies
  (`anomalies_sql`, `crates/debrief-store-duck/src/analysis.rs:849-879`) and field statistics (`:1131-1160`). Patterns,
  exception groups, compare (`analysis.rs:902-903`), spans and the query language stop at `MAX_ANALYSED = 2_000_000`
  (`analysis.rs:31`), Moment at 250,000; each reports a coverage (ADR 0065's B4, `Analyzed`). The in-memory workspace
  (`crates/debrief-core/src/workspace/insights.rs:21`) has the same cap. **The query language refuses** a query that
  matches more than 2M events ("narrow it … first", `crates/debrief-store-duck/src/qlang.rs:66-77`), and known issues
  (`known_issues::evidence`) and playbook queries run through it, so at 1B they fail instead of answering.
- **Gaps.** At the 1B target the capped analyses see 0.2 % of the data, and known issues, playbooks and QL `stats` do
  not run at all on broad queries. Patterns, exception groups and error counts per bucket could be accumulated during
  the import (as ADR 0062 does for the catalogue) and finished per query; counts for known issues and playbook checks
  can be plain store-side counts (no row stream); QL stages can run as SQL over all rows where they translate. Without a
  decision, every usefulness enhancement of the audit is built against a sample and revisited.
- **Target (gap ADR, proposed).** Which analysis is *import-time accumulated*, *store-side over all rows* or
  *sampled with a stated coverage*; a Parallelism and cost section per class; the Finding type of layer I as output.
- **Size.** ADR 3 in 1.2 (design and measurements); the build is 1.3 [J].

### G. Memory bounds

- **State.** MEM-1 and MEM-2 done on `debrief/p0-idle`, MEM-3 to MEM-5 planned, the MEM-6 ADR (capture formats) 3 days;
  ADR 0067's FM gate; the plugin cap.
- **Gaps.** The dumps engine keeps every open document in memory, bounded by count (`MAX_DOCS = 32`,
  `crates/debrief-dump/src/store.rs:13`), not by bytes; a 64-snapshot UMDH series or several heap dumps can exceed any
  sensible budget. UMDH keys are joined strings of up to 64 frames (`crates/debrief-dump/src/umdh.rs:38`).
- **Target.** Widen the MEM-6 ADR's scope to dump and UMDH documents (a byte budget, interned frames as ADR 0069 D2)
  [J: +1 day].
- **Budgets.** The engine's and each plugin's caps; ADR 0069's 64 × 200,000 series within 1 GiB.

### H. Job model

- **State.** Queues per kind (`app/src-tauri/src/jobs/queue.rs`), J1 idempotency planned (shared, waits).
- **Gaps.** Analyses run as bounded queries, not jobs; layer F's store-side analyses and ADR 0069's series need jobs
  with phases, Cancel and "old result stays until the new one is complete". One AI queue means a long research blocks
  chat (audit B20) [S].
- **Target.** J1 and family ADR 0031 (patience); analyses join the Analysis queue (cores − 1) through layer F.

### I. Findings contract — ADR 0068's check report, made shared

- **State** [S]. Findings exist in four shapes: health findings (`analysis/moment.rs:348`), dump and UMDH
  `DumpFinding { kind, severity, title, detail, evidence }` (`crates/debrief-dump/src/api.rs`: `FindingKind` at 391, the struct at about 426-433), profiling findings
  (`crates/debrief-profile/src/ops/findings.rs`), heap findings; report `Block::Finding { severity: String, title, detail }`
  is untyped.
- **Gaps.** Pitwall must explain computed facts only (rule 46), reports must cite them, the AI needs tools over them
  (audit A26). Four shapes mean four adapters each.
- **Target.** ADR 0068's check report (verdict, confidence with reasons, evidence as `EvidenceRef`s, Not checked) as one
  `Finding` type in `debrief-core`, used by every analysis, the report's Finding block and Pitwall's fact bundle. No ADR
  makes it shared yet: a short amendment to ADR 0068, or a section of layer F's ADR.
- **Size.** 1 after netting (the other producers mapped, the report block typed); the contract is ADR 0068 WP-1 [J].

### J. Time model and provenance

- **State.** ADR 0066 (Accepted, Z0 to Z13); the approved `+h:mm` and spaced-date shapes at Checkpoint A; the parser
  accepts offsets of two or four digits only (`crates/debrief-plugin-api/src/time.rs:121,461-478`) [S].
- **Gaps.** Documents' times (dump creation, UMDH snapshot, video, capture start) should carry the same provenance, so a
  rate per hour or a "logs around the snapshot" link is honest (ADR 0069 D5). The query language has no time (audit B8).
- **Target.** ADR 0066 plus a one-line rule: every document time is a provenance-carrying time from ADR 0066's reader.

### K. Logs and diagnostics

- **State.** WP-SL1 (store worker log), X1 bundle privacy, X4 logging ADR (shared).
- **Gaps.** An analysis result cannot be reproduced from a bundle: log the analysis kind, input query hash, coverage,
  Settings hash and duration per run (no user content) [J: 0.5 day, with layer F].
- **Target.** ER-27 and the X4 ADR.

### L. Engine-to-UI contract

- **State.** ADR 0016 command table and generated types; ADR 0063 (binary wire) **Proposed**, its W0 gate 3 days, named
  in §9.6 D2 as the first thing to drop; ADR 0065 `Bounded` on every reply.
- **Gaps.** If W0 is dropped and ADR 0063 adopted later, the reply shapes of 4.5's lists and 3.3's timeline change
  twice. The thread view and report blocks add new list replies.
- **Target.** Decide ADR 0063 (adopt or reject) before 3.3 and 4.5 build their reply shapes; keep W0 if it is the cheapest
  way to decide.

### M. Test corpus and acceptance at scale

- **State.** 4.3 budget corpus, SI-15 bundle acceptance (14,000 files, 7.6 GiB, 1B events nightly), F12, the shapes from
  the bundle-shapes note (invented data).
- **Gaps.** Nothing measures whether analyses **find** things: no corpus with planted incidents (a spike, a leak, a
  restart, a skewed clock, a deadlock dump, a UMDH series) and their expected findings. Without it the usefulness work
  is judged by eye, and hand-tuned weights (audit A13) stay hand-tuned.
- **Target (gap).** A planted-incident generator over the 4.3 corpus, with an expected-findings file and a recall
  check per analysis, nightly (ER-14: invented data only).
- **Size.** 3 [J].

### Planned items that are stop-gaps or would need undoing

| Item | Why | Recommendation |
|---|---|---|
| F10b windowed rows before 2.1's virtual primitive | Two implementations of windowed rows (§9.7 already counts the overlap twice); rule 55 | Build F10b as the first user of 2.1: pull 1.3 → 2.1 ahead of F10 |
| SI-3, plugin text importers "on today's loop" | Interim by design until SI-8/SI-9 | Keep; no new importer features on the old loop |
| UI work on Debrief's own shell before M1–M4 and Y12 | The workbench move carries or rewrites it | Usefulness UI (UMDH series, thread view, report block editors) after M3b, or as components the move carries unchanged |
| Analyses at `MAX_ANALYSED` 2M with a coverage note | A sample against a 1B target | Layer F decides; B4 reports it meanwhile |
| The query language's refusal over 2M matches (`qlang.rs:66-77`), with known issues and playbooks on it | At 1B they fail, not degrade | Layer F: store-side counts for known issues and playbook checks, QL stages in SQL; meanwhile the refusal names the cap through ADR 0065 |
| `u32` event ids on DTOs, `sample_id()` saturating | Wrong events past `u32::MAX`, silently | Layer E's `u64` ids with ADR 0063, before citations and report blocks |
| Report blocks and citations on `u32` ids | Would be redone for layer E | Layer E first |
| UMDH reader fixes on today's whole-file reader | SI-11 replaces that reader | Land them with SI-11, or carry their tests into SI-11's change (ADR 0069 U1) |
| MEM-1/MEM-2 bounds on captures in the UI process | A bound, not a move; MEM-6 decides the move | Keep as the stop-gap it is; widen MEM-6 (layer G) |
| W0 dropped while ADR 0063 stays Proposed | Two reply-shape passes over lists and timeline | Decide ADR 0063 before 3.3 and 4.5 |

## 2. Part 3: proposed roadmap

### 2.1 First: the streaming and folder-open chain, shortened

The chain the user named: 0.5 → 1.4 → 2.6a/2.6b → PH2b → PH4 → 3.4, with ADR 0064's SI-3/4/5/9/10, ADR 0067's F1–F12
and FM, and ADR 0065's UI at about 15,000 rows. §9.3 puts the 1.43 GiB event log at about T+19 and the folder dialog end
to end (F12) at about T+30, with T the first day after the Dyno tag.

What can start earlier or run in parallel [J unless marked]:

1. **Write the waiting packages on branches before T, under conditions.** §9.2 allows it ("may be written on a branch
   before"). But every candidate (1.4 + B1, IP1, 2.6c, PH1, 2.2, 1.3) depends on **0.5**, a 2-day design that is not
   done; PH1 also needs 0.4, which lands only after the tag; wave 0 already uses all seven agents (§9.4); and the Dyno
   pre-tag work changes `crates/` now. So: start only after 0.5 is reviewed, one branch per crate, merged (not rebased)
   after the tag, on spare agents only. The gain is bounded by the working days between 0.5's review and T.
2. **Split 0.5.** Its IP1 and 2.6b part first (about 1 day), so IP1 and 2.6b's design do not wait for the whole 2-day
   design. Gain: about 1 day.
3. **3.4's host side against the test plugin while PH4 migrates the first-party plugins.** §9.2 makes 3.4 + SI-10
   depend on PH2b, **PH4**, SI-8 and 2.6d; 3.4's host-side streaming can be built and tested against
   `debrief-plugin-test` on protocol 2 once PH2b is in, while PH4 + SI-9 move EVTX. Gain: about 2 days. No stop-gap ships.
4. **App-only work of the chain now**: SI-0 to SI-3, SI-6, SI-11 and F1 to F6, FM (all "start now" in §9.4) get the free
   agents first, before any usefulness work.
5. **2.1 before F10** (see the stop-gap table): 1.3 → 2.1 is about 7 days and waits for the tag; start 1.3 on a branch
   now so F10 and F10b build on it. Avoids a second windowed-rows implementation.
6. **4.3's corpus in the first night windows**, so SI-15's acceptance and FM's gate have their data when they need it.

**Chain arithmetic** [calc from §9.2's rows and dependencies; J for overlaps]. §9.3's critical path runs through IP1,
2.6b and 2.6d to 3.4 and leaves out PH4, on which §9.2 makes 3.4 depend. Counting it, the plan's own date is later:

| Step | As planned from T | With 0.5 reviewed and 1.4, IP1, 2.6c, PH1 pre-written |
|---|---|---|
| 0.5 | T+2 | before T |
| 1.4 → 2.6a | T+5 → T+9.5 | lands T+1 → T+5.5 |
| IP1 → 2.6b → SI-8 | T+6 → T+9.5 → T+10.5 | lands T+1 → T+4.5 → T+5.5 |
| 0.4 → PH1 → PH2a | T+1 → T+4.5 → T+7.5 | T+1 → about T+2 → T+5 |
| PH2b (after 2.6a, 2.6b, PH1, PH2a, SI-8) | T+10.5 → T+14 | T+5.5 → about T+9 to T+10 |
| PH4 + SI-9 | T+14 → T+18 | about T+10 → T+14 |
| 2.6d (after 2.6b, 2.6c, IP1) | T+9.5 → T+13.5 | T+4.5 → T+8.5 |
| 3.4 + SI-10: the event log opens | T+18 → **about T+23** | T+14 → **about T+19**; with item 3 (3.4's host side built from PH2b, about T+10), about **T+17** |

So the 1.43 GiB event log opens at about T+19, or about T+17 with item 3, against the plan's stated T+19, which is
about T+23 once PH4 is counted; this holds only if the conditions of item 1 hold. F12 (the folder dialog end to end) moves less: its chain (F1 → F3 → F4 →
F7, F10 on `pd-table`, F11 on SI-4) gains about 2 days from item 5, to about T+28 [J]. The tag date does not move from
this alone (§9.3: capacity, not the path, sets the end).

### 2.2 Then: the foundation additions (1.2)

| # | Package | Days | Depends | Shared code |
|---|---|---|---|---|
| FD-E1 | Evidence reference ADR (layer E) | 2 | ADR 0051, 0066 | design |
| FD-U | `EventId` (u64) on every DTO, generated TypeScript and wire; `sample_id()` removed | 3 | ADR 0063 decided | no (app crates; the wire encoding with ADR 0063) |
| FD-P | `ProcessInstance` type in `debrief-core` and UMDH on it (netted: the dumps' same-process check is ADR 0068 WP-6, which builds on this type) | 0.5 | ADR 0068 §1 | no |
| FD-E2 | `EvidenceRef`, resolver, notes/reports/AI moved | 3 | FD-E1, FD-U, FD-B | no (app crates) |
| FD-B | Ordinal per source: `line` for text, a declared ordinal for the rest, uniqueness at the sink (ADR 0062 amendment) | 1.5 | IX-B1 | no |
| FD-I | ADR 0068's `CheckReport` made the shared finding: health, profiling and heap findings mapped, the report's Finding block typed (netted: the contract itself is ADR 0068 WP-1) | 1 | ADR 0068 WP-1 | no |
| FD-F | Analyses at scale ADR with measurements | 3 | Q-1, 4.3 | design |
| FD-G | MEM-6 scope widened to dump and UMDH documents | 1 | MEM-6 | design |
| FD-K | Analysis run log lines | 0.5 | FD-F | no |
| FD-M | Planted-incident generator and recall check | 3 | 4.3 | the bench workflow line waits |
| | **Total** | **18.5** (after netting 2 against ADR 0068) | | |

### 2.3 Then: usefulness in 1.2 (approved items and tiny ones)

Approved items without a size in the plan, priced here [J] (replace with their ADRs' numbers when written):

| Item | Days [J] | Depends | Note |
|---|---|---|---|
| Time shapes `+h:mm` and spaced dates (Checkpoint A) | 1.5 | Z1 | tiny, app only; may run beside the chain now |
| Citations | 3 | FD-E2 | on top of layer E |
| Report blocks for analysis results | 5 | FD-I, FD-E2 | engine-rendered |
| Pitwall report drafting | 4 | FD-I, report blocks, ADR 0059 | facts only (rule 46) |
| Thread/sequence view | 7 | FD-B, FD-P, 2.4/3.3 lanes | after M3b for its UI |
| ADR 0068, dump checks, native (all WPs but WP-0 and WP-12) | 81 [P] | - | Accepted 2026-11-07 (`1ba8e4ecd`) with its whole scope in 1.2 |
| ADR 0068, .NET managed state (WP-0 3 + WP-12 15) | 18 [P] | WP-0, WP-1, WP-RR | the approved ".NET dump stacks" |

Tiny usefulness items that do not compete with the chain (app only, small files, no shared code): UMDH reader fixes and
test generator (2, landing with SI-11), known issues and playbooks scoped to the window (1), CLI `--profile` (1), the
collector's detector set from the share's Setting (1), outliers' Benjamini–Hochberg (0.5), watched-folder and range
import in local and relative time (1, with ADR 0066's Z packages), SQL result as a report block (0.5). **7 days.**

### 2.4 1.3 and later

- **1.3**: UMDH series (ADR 0069 rest, about 25), analysis thresholds as Settings (2), compare with pattern compare,
  significance and evidence (3), time in the query language (4), anomaly scores and per-operation baselines (4), AI
  tools over every analysis (3), Moment ranking and baselines (2), exception and pattern trends and spread samples (3.5),
  runs percentiles (1.5), memory growth on the shared trend classifier (1.5), traces by operation (2.5), playbook
  parameters (2), config diff by key and on the timeline (2), Explorer coverage (1.5), lens with time and instances (1),
  inspector chained correlation (2), folder survey in the scan (2), layer F's build (judged after its ADR), notes with
  several anchors (2), agent manifest for UMDH times (1), profiling thresholds in Settings (1.5). About **67.5** [calc
  from the audit rows], plus layer F's build once its ADR sizes it.
- **Later**: clock drift (3), sequence operator in QL (4), alert hooks and history (3), plugin analysis contributions
  (3), chart overlays and small multiples (2), field statistics over time (1.5), dashboard as a report block (1), mixed
  encodings (1), research links (1), related-logs weights from the corpus (1). About **20**.

### 2.5 Effect on the 1.2 end date

| Added to 1.2 | Raw days | With 20 % |
|---|---|---|
| Foundation additions (2.2), netted | 18.5 | 22.2 |
| Approved items without a size elsewhere (time shapes, citations, report blocks, Pitwall drafting, thread view) | 20.5 | 24.6 |
| ADR 0068, Accepted, whole scope (81 native + 18 .NET) | 99 | 118.8 |
| Tiny usefulness items | 7 | 8.4 |
| **Total** | **145** | **174** [calc] |

Netted: 2 days, FD-P against ADR 0068 WP-6 (same-process check) and FD-I against WP-1 (the `CheckReport` contract); 1
each. Without netting it is 147 raw, 176.4 with the buffer.

At §9.4's planning number of 5.5 agent-days a working day, 174 / 5.5 is about **32 working days**: the end moves from
about **2027-04-28** (§9.5) to about **2027-06-11** [calc; J for the sizes; no holidays counted]. The chain speed-ups of
2.1 do not shorten the end (capacity sets it), but they bring the user's first step (the large event log opens) to about
T+19, or T+17 with item 3, against about T+23 for the plan's own rows once PH4 is counted.

## 3. Decisions (decided by the user, 2026-11-07)

Decided as recommended: R1 (with its conditions), R2, R2b (a declared bound under 2^53 recommended; the final choice
with ADR 0063), R2c, R3, R4, R5, R6 (UMDH reader fixes in 1.2 with SI-11, the redesign later), R8, R9, and ADR 0069's
D-b to D-h. **R7 was first decided as "keep everything in 1.2" (end about 2027-06-11) and then replaced the same day by
the user's direction "solid foundation in 1.2 first, then slowly one function one by one"**: section 4, Release split.
The table keeps the recommendations as they were put to the user.

| # | Decision | Recommendation |
|---|---|---|
| R1 | Write the chain's waiting packages (1.4 + B1, IP1, 2.6c, PH1, 2.2, 1.3) on branches before the Dyno tag | **Yes, conditionally**: only after 0.5 is reviewed, one branch per crate, merged (not rebased) after the tag, on spare agents only; the gain is bounded by the working days between 0.5's review and T (2.1's table) |
| R2 | Layer E, one evidence reference for notes, reports, AI and findings, before citations and report blocks | **Yes, in 1.2** (5 days); otherwise citations are built twice |
| R2b | `u64` event ids on every DTO and wire (FD-U), and when | **In 1.2, decided with ADR 0063 and done before citations and report blocks** (3 days); for the TypeScript side, a declared bound (ids < 2^53, checked at the sink) is the cheapest of string, bigint or bound |
| R2c | One `ProcessInstance` (pid, start time, main image identity) for dumps, UMDH, the thread view and evidence references (FD-P) | **Yes, in 1.2** (0.5 after netting with ADR 0068 WP-6; was 1.5 days), with ADR 0068 |
| R3 | Layer F, an ADR on analyses at 1B events, in 1.2; its build in 1.3. Scope includes the query language's 2M refusal and known issues and playbooks on store-side counts | **Yes** (3 days); if the refusal must go in 1.2, the store-side counts for known issues and playbooks first (about 1.5 days [J]) |
| R4 | Make ADR 0068's check report the one shared `Finding` (layer I) | **Yes** (2 days) |
| R5 | Planted-incident corpus (layer M) | **Yes, in 1.2** (3 days): the only way to measure usefulness |
| R6 | ADR 0069 (UMDH series): reader fixes in 1.2 with SI-11, the rest in 1.3; minimum snapshots and caps as its D-g (no leak verdict from two logs (`Grew` at most `Medium`, as ADR 0068); `Low` at 3, `Medium` at 4–5, `High` from 6 only when every step grew; 64 snapshots, 200,000 stacks, 1,000,000 frames) | **Yes** |
| R7 | If the date matters, what to defer to 1.3 (ADR 0068 was accepted with its whole scope in 1.2, so deferring any of it reverses that decision) | From ADR 0068's own list, the separable packages: **.NET managed state, WP-0 + WP-12, 18**; capture presets and flags, WP-11, 3; the unexpected-modules pack, WP-16, 2. Then outside it: the thread/sequence view, 7, and Pitwall drafting, 4. All five: 34 raw, 40.8 with the buffer, about 7.4 working days, so the end about **2027-06-01** [calc]. .NET alone: 21.6 with the buffer, about 4 working days (about 2027-06-07). Not deferrable without rework: WP-1, WP-2, WP-RR, WP-8 and WP-3 (the base of every check), WP-17 (it removes a second minidump reader, rule 55), WP-9 (events around the dump, the link to the logs). Keep layers E and I and the report blocks, which the rest needs |
| R8 | Keep or drop W0 (§9.6 D2) | Keep, but only if ADR 0063 is decided before 3.3 and 4.5 build their replies; else drop and reject ADR 0063 for 1.2 |
| R9 | Usefulness UI before or after the workbench move (M3b, Y12) | After, except the tiny items |

## 4. Release split (the user's direction, 2026-11-07)

The user chose, on 2026-11-07: "solid foundation in 1.2 first, then slowly one function one by one". This section
replaces R7 and section 2.5's end date. It changes **release placement only**; every design (ADRs 0064 to 0069, the
layers of section 1) stays as accepted.

### 4.1 Debrief 1.2: foundation, plus bug fixes and correctness

Rules used: **foundation** is what later functions stand on (import, store, plugin host, folder open, memory, jobs,
time, logs, the engine-to-UI contract, shell and lists, test corpus, family obligations a tag needs); **correctness** is
a wrong result or a broken path today; **function** is a new capability for the investigator. Functions leave 1.2,
unless a foundation acceptance test needs them (then they stay and are named).

| Class | Packages (plan §9.2 and the scope plan's rows, plus this note's FD items) | Raw days |
|---|---|---|
| Foundation, plan | Layers 0 to 6 of §9.2: L-0, X10, X11a/b, X1, WP-SL1, 0.2 to 0.5, X7; 1.4 + B1, 2.2, P1, IP1, 2.6a to 2.6d, PH1 to PH4, J1, 1.2 rest, 1.3, M1 to M4, D-a; SI-0 to SI-18; MEM-3 to MEM-5, MEM-6 ADR; Z0 to Z13 (ADR 0066); F1 to F12, FM, FR (ADR 0067); B0 to B8 (ADR 0065); ST, Q-1 to Q-4, IX, SR, CPU-1, NB-1, P4, BP1, 4.3, 5.1, 5.2, D-b, the bench gates; 2.1, 2.3 to 2.5, 2.7, 3.2, 3.3, 3.6, 3.7, 4.1, 4.2, 4.4, 4.5, the drawing build, W0, LV1 to LV3; Y3 rest, Y4, Y6, Y7, Y8, Y11, Y12, Y14's R0 and R3 (6), Y16; X2 to X6, X8, X9; N3 (the packs mechanism the frame classifier and ADR 0068's unexpected-modules pack use), N8; 5.4, M5, F, R | 463.5 [calc: §9.5's 534 less the functions below] |
| Foundation, new | FD-U 3, FD-P 0.5, FD-E1 2, FD-E2 3, FD-B 1.5, FD-I 1, FD-F 3, FD-G 1, FD-K 0.5, FD-M 3; **plus ADR 0068's WP-1 (4)**: FD-I rests on its `CheckReport` contract, so the contract stays in 1.2 while the dump checks leave | 22.5 |
| Correctness | UMDH reader fixes and test generator (2, with SI-11); `+h:mm` and spaced-date shapes (1.5, Checkpoint A); known issues and playbooks scoped to the window (1; they count outside it today); CLI `--profile` (1; the CLI ignores the profile today); outliers' Benjamini–Hochberg (0.5; p-values over up to 6,000 tests); the cap-stop and offset fix and the AI coverage fix (already done, 0) | 6 |
| **1.2 total** | | **492 raw, 590.4 with 20 %** [calc] |

**Out of 1.2 (functions):** from the plan Y1 AI memory (10.5), Y2 semantic search with X55 (9.5), N4, N5, N0, N7
(5.5), Y5 tray popup (5), Y9's small items (4), Y10 rule 47 in Debrief's views (18.5), Y13 formats, PML, batch queries
(9), Y14's R5 UI items (8): **70.5**. From the approved items: citations, report blocks, the thread/sequence view,
Pitwall drafting and all of ADR 0068 except WP-1; from the tiny list the collector's detector set, range import in
local time, the SQL result block.

- **Y10 needs a check** [U]: if a conformance gate or a waiver with a date (C-checks, `paddock.waivers.toml`) requires
  rule 47 in Debrief's views before its next tag, Y10 (or the part the gate checks) comes back into 1.2.
- Nothing a foundation acceptance test needs is stranded: SI-15 and F12 need the import, the dialog and the store;
  the B packages need the lists; FD-M needs only the corpus; FD-E2's tests use notes, which exist.

**End date** [calc]: 590.4 / 5.5 = 107.3 working days, plus Spotter 2.5 and quiet-window slips 4 (§9.5's
allowances): about **114 working days from 2026-11-09, so about 2027-04-15**, against about 2027-04-28 for the plan as it
stood and about 2027-06-11 for "keep everything". 1.2 is barely shorter than the plan because the plan was already
mostly foundation; the gain is that the functions after it no longer wait for each other.

### 4.2 After 1.2: one function per release

Ordered by value for effort and dependencies. Each release: raw days × 1.2 spread over the agents one function can use
(about 2.5 agent-days a day for a small function, 3.4 to 4.25 for the dump and UMDH work [J]), plus about 2 working days
of release work (safety net, VM pass, tag) [J]. Dates are working days after 2027-04-15, no holidays, one release at a
time (ER-1) [calc].

| Release | Function | Raw days | Working days | Rough date |
|---|---|---|---|---|
| 1.3 | Citations and report blocks (with the SQL result block) | 8.5 | 6 | 2027-04-26 |
| 1.4 | Dump checks, crash and hang: ADR 0068 native WP-2, WP-RR, WP-8, WP-3, WP-4, WP-5, WP-6, WP-9, WP-10, WP-11, WP-13, WP-17, WP-18, most of WP-14 and WP-15 | 67 | 21 | 2027-05-25 |
| 1.5 | Thread/sequence view | 7 | 5.5 | 2027-06-02 |
| 1.6 | Pitwall report drafting | 4 | 4 | 2027-06-08 |
| 1.7 | Dump check, memory: WP-7, WP-16, the rest of WP-14 and WP-15 | 10 | 7 | 2027-06-17 |
| 1.8 | UMDH series (ADR 0069 less the reader fixes) | 25 | 11 | 2027-07-02 |
| 1.9 | .NET managed state in dumps (ADR 0068 WP-0, WP-12) | 18 | 10.5 | 2027-07-19 |
| 1.10 | Analyses at 1B events, the R3 build [J: sized by its ADR] | 10 | 7 | 2027-07-28 |
| 1.11 | Analysis thresholds as Settings, and compare with pattern compare, significance and evidence | 5 | 4.5 | 2027-08-04 |
| 1.12 | Time in the query language | 4 | 4 | 2027-08-10 |
| 1.13 | Anomaly scores and per-operation baselines | 4 | 4 | 2027-08-16 |
| 1.14 | AI tools over every analysis | 3 | 3.5 | 2027-08-20 |
| 1.15 | Semantic search (Y2) | 9.5 | 6.5 | 2027-08-31 |
| 1.16 | AI memory (Y1) | 10.5 | 7 | 2027-09-09 |
| 1.17 | Rule 47 in Debrief's views (Y10), unless a gate pulls it into 1.2 | 18.5 | 8.5 | 2027-09-22 |

Then, one each in the same way: names (N4, N5, N0, N7, 5.5), formats, PML and batch queries (Y13, 9), the tray popup
(Y5, 5), Y14's R5 UI items (8), Y9's small items (4), and the rest of the audit's 1.3 list (section 2.4: Moment,
exception and pattern trends, runs, traces, playbook parameters, config diff, Explorer coverage, lenses, inspector,
folder survey in the scan, notes with several anchors), then its "later" list.

- **Why this order.** Citations and report blocks first: small, and every later function reports through them. Dump
  crash and hang next: the user's own complaint and the largest gain, on 1.2's `CheckReport` and `ProcessInstance`.
  The thread view and Pitwall drafting are small and lean on the first two. The memory check, UMDH and .NET follow the
  dump chain's dependencies (WP-6 before WP-7; ADR 0069 uses ADR 0068's report and the memory check's link; WP-12 after
  WP-0 and WP-RR). The analyses at scale come before the analysis enhancements that would otherwise be built twice.
- **Capacity.** One function at a time uses 2 to 5 agents; the rest of the family's capacity is free for the other apps
  (ER-1). Designs and failing tests for the next release may be written while the current one finishes.

### 4.3 What this changes

- **Release placement only.** R7's "keep everything in 1.2" is replaced. The approved items (citations, report blocks,
  thread/sequence view, Pitwall drafting, .NET dump stacks) move to 1.3 to 1.9; their designs do not change.
- **ADR 0068** (Accepted, `1ba8e4ecd`, scope "Debrief 1.2"): its scope line needs updating to "WP-1 in 1.2; crash and
  hang in 1.4; memory in 1.7; .NET in 1.9". That edit belongs to ADR 0068's branch and author; it is not made here.
- **ADR 0069**: its status line already says the reader fixes are in 1.2 and the rest in its own release (1.8 here).
- **The 1.2 plan** (`docs/plans/debrief-1-2.md`, §9): its totals and the packages listed as out above change when the
  coordinator adopts this split; this note does not edit the plan.
- **Section 2.3 and 2.5 above** describe the "everything in 1.2" option the user first chose; they stay as the record of
  that option.
