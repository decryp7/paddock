# Debrief: foundation first, then usefulness (roadmap proposal, 2026-11-07)

A proposal, not a plan change: `docs/plans/debrief-1-2.md` is not edited. It builds on the usefulness audit
[2026-11-07-analysis-usefulness.md](2026-11-07-analysis-usefulness.md) (rows A1–A27, B1–B27 cited below) and on the plan
refresh, section 9 of `docs/plans/debrief-1-2.md` on `docs/debrief-1-2-plan-refresh` `abac43c0c` ("§9" below).

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

- **State.** DuckDB 2 with Parquet segments and Tantivy; event ids are `u64` blocks per segment, never reused
  (`crates/debrief-store-duck/src/writer.rs:755`, `segments.rs:3`); ADR 0062 builds index and catalogue during the
  import; ST-1 to ST-6 and Q-1 to Q-4 are planned (§9.2 layer 2).
- **Gaps.** The thread/sequence view needs a total order inside a source (record ordinal as tie-break for equal
  timestamps) and typed thread, process and activity columns; exact positions are decided only below 100M (§9.6 D7).
  Without an ordinal in the store, a sequence view at 1B events is built on an approximation and redone later [J].
  Field values are not in the full-text index (`fts.rs`) [S].
- **Target.** ST and IX as planned, **plus** a declared `seq` (source id, record ordinal) column written by the sink:
  no ADR covers it today (gap; fold into ADR 0062 as an amendment, about 1 day of design, 1.5 of build) [J].
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

### E. Evidence references (citations) — no ADR yet

- **State** [S]. Three types point at an event: AI `Citation::Record { id: u32, verified }`
  (`crates/debrief-ai/src/assistant/citations.rs:11`), report `Block::Events { ids: Vec<u32> }`
  (`crates/debrief-core/src/report/mod.rs:108`), note anchors `Event { event_id: u64, time, source, excerpt }`
  (`crates/debrief-notes/src/anchor.rs`). The store's ids are `u64`. `EventsRequest` leaves out ids that no longer exist
  without saying so (`crates/debrief-core/src/ports/dto.rs:87`). Whether a reload or a re-time keeps event ids is not
  traced [U]; ADR 0051 keeps **source** ids stable, not event ids.
- **Gaps.** The approved citations and report blocks, Pitwall drafting and every finding's evidence need one reference
  that survives reload, re-time (ADR 0066's Undo), a redacted copy and a project moved without its logs, and says
  "moved" or "gone" instead of vanishing. Building citations on the `u32` ids means undoing them later.
- **Target (gap ADR, proposed).** One `EvidenceRef` in `debrief-core`: stable source id (ADR 0051), a locator (record
  ordinal from layer B, byte offset and line for text, document key for a dump, UMDH call site or capture node), a
  content hash, the time with its provenance, and an id hint for the fast path. Resolution answers *found*, *moved*
  (resolved by locator and hash) or *gone* (with the kept excerpt), in ADR 0065's `Bounded` style. Notes, report blocks,
  AI citations and findings all use it (rule 55: one mechanism, three copies today).
- **Budgets.** Resolution of a 1,000-reference report under 200 ms on the 1B store [J target].
- **Size.** ADR 2, build 3 (types, resolver, the three users moved) [J].

### F. Analyses at scale — no ADR yet

- **State** [S]. Log analyses read at most `MAX_ANALYSED = 2_000_000` events (`crates/debrief-core/src/workspace/insights.rs:21`)
  and Moment 250,000, each with a coverage note; ADR 0065's B4 (`Analyzed`) reports it; Q-2 makes results pageable.
- **Gaps.** At the 1B target an analysis sees 0.2 % of the data. Patterns, exception groups, error counts per bucket and
  field facets could be accumulated during the import (as ADR 0062 does for the catalogue) and finished per query; the
  rest run as store-side SQL over all rows, as jobs with progress (ER-17 to ER-20). Without a decision, every usefulness
  enhancement of the audit is built against a sample and revisited.
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
  `DumpFinding { kind, severity, title, detail, evidence }` (`crates/debrief-dump/src/api.rs:391-433`), profiling findings
  (`crates/debrief-profile/src/ops/findings.rs`), heap findings; report `Block::Finding { severity: String, title, detail }`
  is untyped.
- **Gaps.** Pitwall must explain computed facts only (rule 46), reports must cite them, the AI needs tools over them
  (audit A26). Four shapes mean four adapters each.
- **Target.** The dump analysis ADR's finding (verdict, confidence, evidence as `EvidenceRef`s, could-not-check) as one
  `Finding` type in `debrief-core`, used by every analysis, the report's Finding block and Pitwall's fact bundle. No ADR
  makes it shared yet: a short amendment to ADR 0068, or a section of layer F's ADR.
- **Size.** 2 (type, the four producers mapped, report block) [J].

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
| Report blocks and citations on `u32` ids | Would be redone for layer E | Layer E first |
| MEM-1/MEM-2 bounds on captures in the UI process | A bound, not a move; MEM-6 decides the move | Keep as the stop-gap it is; widen MEM-6 (layer G) |
| W0 dropped while ADR 0063 stays Proposed | Two reply-shape passes over lists and timeline | Decide ADR 0063 before 3.3 and 4.5 |

## 2. Part 3: proposed roadmap

### 2.1 First: the streaming and folder-open chain, shortened

The chain the user named: 0.5 → 1.4 → 2.6a/2.6b → PH2b → PH4 → 3.4, with ADR 0064's SI-3/4/5/9/10, ADR 0067's F1–F12
and FM, and ADR 0065's UI at about 15,000 rows. §9.3 puts the 1.43 GiB event log at about T+19 and the folder dialog end
to end (F12) at about T+30, with T the first day after the Dyno tag.

What can start earlier or run in parallel [J unless marked]:

1. **Write the waiting packages now, on branches.** §9.2 allows it ("may be written on a branch before"). 1.4 + B1,
   IP1, 2.6c, PH1 and 2.2 have no app dependency beyond 0.5; written before T they land in the first days after it
   instead of starting then. Gain on the path: about 3 to 5 days (IP1 4 and 1.4 3 are on or next to the path).
2. **Split 0.5.** Its IP1 and 2.6b part first (about 1 day), so IP1 and 2.6b's design do not wait for the whole 2-day
   design. Gain: about 1 day.
3. **3.4 + SI-10 against 2.6b's interface while 2.6d is built**, with 2.6d's judge behind its trait and a test double;
   integrate when 2.6d lands. Gain: about 2 to 3 days of 3.4's 5. No stop-gap ships: the double lives in tests.
4. **App-only work of the chain now**: SI-0 to SI-3, SI-6, SI-11 and F1 to F6, FM (all "start now" in §9.4) get the free
   agents first, before any usefulness work.
5. **2.1 before F10** (see the stop-gap table): 1.3 → 2.1 is about 7 days and waits for the tag; start 1.3 on a branch
   now so F10 and F10b build on it. Avoids a second windowed-rows implementation.
6. **4.3's corpus in the first night windows**, so SI-15's acceptance and FM's gate have their data when they need it.

Effect [J]: the event log opens at about **T+14 to T+16** instead of T+19, and F12 at about **T+26** instead of T+30, if
the pre-T branches are written now and land cleanly. The tag date does not move from this alone (§9.3: capacity, not the
path, sets the end).

### 2.2 Then: the foundation additions (1.2)

| # | Package | Days | Depends | Shared code |
|---|---|---|---|---|
| FD-E1 | Evidence reference ADR (layer E) | 2 | ADR 0051, 0066 | design |
| FD-E2 | `EvidenceRef`, resolver, notes/reports/AI moved | 3 | FD-E1; layer B's `seq` | no (app crates) |
| FD-B | `seq` ordinal column (ADR 0062 amendment and build) | 2.5 | IX-B1 | no |
| FD-I | One `Finding` type (ADR 0068's shape) for all analyses and the report block | 2 | ADR 0068 | no |
| FD-F | Analyses at scale ADR with measurements | 3 | Q-1, 4.3 | design |
| FD-G | MEM-6 scope widened to dump and UMDH documents | 1 | MEM-6 | design |
| FD-K | Analysis run log lines | 0.5 | FD-F | no |
| FD-M | Planted-incident generator and recall check | 3 | 4.3 | the bench workflow line waits |
| | **Total** | **17** | | |

### 2.3 Then: usefulness in 1.2 (approved items and tiny ones)

Approved items without a size in the plan, priced here [J] (replace with their ADRs' numbers when written):

| Item | Days [J] | Depends | Note |
|---|---|---|---|
| Time shapes `+h:mm` and spaced dates (Checkpoint A) | 1.5 | Z1 | tiny, app only; may run beside the chain now |
| Citations | 3 | FD-E2 | on top of layer E |
| Report blocks for analysis results | 5 | FD-I, FD-E2 | engine-rendered |
| Pitwall report drafting | 4 | FD-I, report blocks, ADR 0059 | facts only (rule 46) |
| Thread/sequence view | 7 | FD-B, 2.4/3.3 lanes | after M3b for its UI |
| .NET dump stacks | 11 [P] | ADR 0068 | ADR 0068's WP-0 and WP-12 (its total 63, native checks 52 without them) |
| Dump checks, native (ADR 0068 without .NET) | 52 [P] | ADR 0068 | not in the totals below; shown as a second line in 2.5 |

Tiny usefulness items that do not compete with the chain (app only, small files, no shared code): UMDH reader fixes and
test generator (1.5), known issues and playbooks scoped to the window (1), CLI `--profile` (1), collector on the app's
redaction (1), outliers' Benjamini–Hochberg (0.5), watched-folder and range import in local and relative time (1, with
ADR 0066's Z packages), SQL result as a report block (0.5). **6.5 days.**

### 2.4 1.3 and later

- **1.3**: UMDH series (ADR 0069 rest, about 23.5), analysis thresholds as Settings (2), compare with pattern compare,
  significance and evidence (3), time in the query language (4), anomaly scores and per-operation baselines (4), AI
  tools over every analysis (3), Moment ranking and baselines (2), exception and pattern trends and spread samples (3.5),
  runs percentiles (1.5), memory growth on the shared trend classifier (1.5), traces by operation (2.5), playbook
  parameters (2), config diff by key and on the timeline (2), Explorer coverage (1.5), lens with time and instances (1),
  inspector chained correlation (2), folder survey in the scan (2), layer F's build (judged after its ADR), notes with
  several anchors (2), agent manifest for UMDH times (1), profiling thresholds in Settings (1.5). About **66** [calc
  from the audit rows], plus layer F's build once its ADR sizes it.
- **Later**: clock drift (3), sequence operator in QL (4), alert hooks and history (3), plugin analysis contributions
  (3), chart overlays and small multiples (2), field statistics over time (1.5), dashboard as a report block (1), mixed
  encodings (1), research links (1), related-logs weights from the corpus (1). About **20**.

### 2.5 Effect on the 1.2 end date

| Added to 1.2 | Raw days | With 20 % |
|---|---|---|
| Foundation additions (2.2) | 17 | 20.4 |
| Approved items priced here (2.3, without ADR 0068's native checks) | 31.5 | 37.8 |
| Tiny usefulness items | 6.5 | 7.8 |
| **Total** | **55** | **66** [calc] |
| ADR 0068's native checks, if all in 1.2 | 52 | 62.4 |

At §9.4's planning number of 5.5 agent-days a working day that is about **12 working days**: the end moves from about
**2027-04-28** (§9.5) to about **2027-05-14** [calc; J for the sizes]. With all of ADR 0068's native checks in 1.2 as
well, about 11 more working days: about **2027-05-31**. The chain
speed-ups of 2.1 do not shorten the end (capacity sets it), but they bring the user's first step (a large folder opens
cleanly) forward by about 3 to 4 days.

## 3. Decisions for the user

| # | Decision | Recommendation |
|---|---|---|
| R1 | Write the chain's waiting packages (1.4 + B1, IP1, 2.6c, PH1, 2.2, 1.3) on branches now, landing after the Dyno tag | **Yes**: the cheapest way to open a large folder sooner |
| R2 | Layer E, one evidence reference for notes, reports, AI and findings, before citations and report blocks | **Yes, in 1.2** (5 days); otherwise citations are built twice |
| R3 | Layer F, an ADR on analyses at 1B events, in 1.2; its build in 1.3 | **Yes** (3 days) |
| R4 | Make ADR 0068's check report the one shared `Finding` (layer I) | **Yes** (2 days) |
| R5 | Planted-incident corpus (layer M) | **Yes, in 1.2** (3 days): the only way to measure usefulness |
| R6 | ADR 0069 (UMDH series): reader fixes in 1.2, the rest in 1.3 | **Yes** |
| R7 | If the date matters, defer first: part of ADR 0068 (its own D-list says which checks), then .NET dump stacks (11) and the thread/sequence view (7) to 1.3, then Pitwall drafting (4) | .NET plus the thread view save about 21.6 days with the buffer (about 3.9 working days), with Pitwall drafting about 26.4 (about 4.8) [calc]; keep layers E and I and the report blocks, which the rest needs |
| R8 | Keep or drop W0 (§9.6 D2) | Keep, but only if ADR 0063 is decided before 3.3 and 4.5 build their replies; else drop and reject ADR 0063 for 1.2 |
| R9 | Usefulness UI before or after the workbench move (M3b, Y12) | After, except the tiny items |
