# Debrief: how useful is each function to an investigator (audit, 2026-11-07)

Docs only: nothing was built or run (the machine was loaded). The roadmap that slots these findings into 1.2, 1.3 and
later, and the foundation they rest on, is the companion note
[2026-11-07-roadmap-proposal.md](2026-11-07-roadmap-proposal.md).

- Basis: `origin/debrief/1.2-int` `852b35ac0`; paths under `apps/debrief/` unless they start with `docs/` (repo root).
- Marks: **[S]** read in source; **[D]** official documentation, read 2026-11-07, URL given; **[J]** judgement;
  **[U]** not verified (a grep that found nothing, or a claim not traced through the whole path). Sizes are judgement,
  raw agent-days, ±50 %.
- Ratings: **useful** (an investigator gets an answer they would act on), **basic** (correct, but they still have to do
  the real work by hand or in another tool), **not useful** (as built, it rarely changes what they do).
- Revision 2 (2026-11-07) answers the independent review of `cee49f7a1`.
- "In plan": the 1.2 plan (`docs/plans/debrief-1-2.md`; its section 9 is **not** on the base `852b35ac0`, only on
  `docs/debrief-1-2-plan-refresh` `abac43c0c`) or the
  items the user approved since (citations, report blocks, thread/sequence view, Pitwall report drafting, .NET dump
  stacks, `+h:mm` and spaced-date time shapes at Checkpoint A, the dump analysis ADR).
- Dumps: the user's "the open dump file function is too basic" is the subject of ADR 0068 (dump checks, Proposed, on
  `docs/debrief-dump-analysis-adr`); this audit does not redo it. The UMDH design below returns ADR 0068's `CheckReport`
  shape and defers to it where they differ.
- No customer, host or confidential folder names; every example is invented.

## Part 1: UMDH diff

The design is [ADR 0069](../adr/0069-umdh-leak-analysis-over-a-series.md) (Proposed). This part is its summary.

### What it does today [S]

| Layer | Code | What it does | Limits |
|---|---|---|---|
| Sniff and read | `crates/debrief-plugin-dumps/src/umdh_reader.rs:26,70,200` | Recognises a snapshot (`<size> bytes + <overhead> at <address> by BackTrace<id>` per allocation) and a UMDH diff (`± delta (new - old) n allocs BackTrace<id>`, count line, stack, `Total increase`), hex or `-d` decimal (decided by the total line), UTF-8 or UTF-16 with a BOM. Sums per back trace: bytes, count, largest. Frames `module!function+off`, `module+off` or raw addresses placed in the listed modules | Whole file in memory (`read_exact(0, len)`, ≤ `max_read_bytes` 1 GiB); 200,000 stacks; 64 frames per stack; text ≤ 1,024 chars (`crates/debrief-dump/src/limits.rs:35-46`); lines ≤ 4,096 (`MAX_LINE`, `umdh_reader.rs:23`) |
| Time and process | `umdh_reader.rs:330-333` | Time = the file's modification time; process never filled in | - |
| Symbols | `crates/debrief-plugin-dumps/src/import.rs:39`; ADR 0036 §2 | None: "the names UMDH wrote" | - |
| Growth | `crates/debrief-dump/src/umdh.rs:94` | Two logs, stacks matched by full frame list (`module!function` per frame joined into one string), sorted by byte growth | One `(base, target)` pair cached (`crates/debrief-dump/src/store.rs:205`) |
| Grouping | `umdh.rs:124` | By stack, module, or function of the culprit frame (first frame that is neither an allocator nor system or runtime code, `crates/debrief-dump/src/modules.rs:106,140`); substring filter; paged | - |
| Tree | `umdh.rs:224` | Top-down tree of growth | - |
| Findings | `umdh.rs:304` | Heap total first vs last; top 5 growing stacks ≥ 1 MiB or ≥ 10 % (critical ≥ 50 %); "steady growth" if bytes rise strictly in **every** step of ≥ 3 snapshots; single allocations ≥ 16 MiB | Constants, not Settings (`umdh.rs:283-288`) |
| Explain | `crates/debrief-dump/src/explain.rs:71` | Prompt with file names, counts and the findings | Bounded text |
| UI | `app/src/app/dumps/umdh-view.ts` | Log picker plus "compared with"; tabs Findings, Growth (Growth, Allocations, Alive, Stacks), Tree; stack panel marks "allocated here" | No series view, no export, no settings |

Also: the UMDH reader runs in the app process for the sniff and summary path (`app/src-tauri/src/dumps.rs:557`), which
ADR 0064 row 6 lists among the whole-file readers its SI-11 moves.

### Why it is weak for finding a leak

Rating: **basic, close to not useful** for its main job [J]. In short (details and sources in ADR 0069's Context):

1. Two snapshots cannot tell a leak from a warming cache; the tools investigators use reason over time (WPA's
   allocated-inside/freed-outside classes, [WPA heap glossary](https://learn.microsoft.com/en-us/previous-versions/windows/desktop/xperf/wpa-heap-glossary);
   DebugDiag's top allocators with sizes, [Debug Diag 1.1 walkthrough](https://learn.microsoft.com/en-us/archive/blogs/tess/debugging-native-memory-leaks-with-debug-diag-1-1);
   UMDH's own guide, [Using UMDH](https://learn.microsoft.com/en-us/windows-hardware/drivers/debugger/using-umdh-to-find-a-user-mode-memory-leak)) [D].
2. The series test is strict (`umdh.rs:383`): one flat step removes a real leak from "steady growth". The size test
   (`LeakGrowth`, first against last) still fires, but it fires the same way for a one-off load step, so a leak and a
   step look alike.
3. Identity by the whole stack splits one call site reached from many callers into rows below the 10 % threshold.
4. No names for raw addresses, and no statement of how much growth is unnamed.
5. Times are file modification times, shown nowhere; ordering and any rate rest on them.
6. Per-allocation sizes are discarded; "all growth is 1,064-byte blocks" is lost.
7. No context: Private Bytes against the NT heap, logs at snapshot times, .NET heap.
8. Findings do not say what they could not check.

Two **wrong results** found on the way, each with a failing test in ADR 0069 [S against the documented shape, U
against a real file]:

- The documented aggregated entry `00005320 bytes in 0x14 allocations (@ 0x00000428) by: BackTrace00B53`
  ([Using UMDH](https://learn.microsoft.com/en-us/windows-hardware/drivers/debugger/using-umdh-to-find-a-user-mode-memory-leak))
  is read as **one** allocation of 0x5320 bytes (`umdh_reader.rs:284-291`), so count and largest are wrong.
- Frames that are not indented are dropped (`umdh_reader.rs:300`), so the stack has no frames: no culprit, no names,
  no grouping by module or function. Its key falls back to `#<id>` (`umdh.rs:39-41`); a `BackTrace` id is stable within
  one process lifetime, so matching still works there, but after a restart the same id can be a different stack and
  would be merged silently.

UMDH quirks the design handles [D]: `gflags /i <image> +ust` and a restart before snapshots; the stack database cap
(32 MB x86, 64 MB x64 by default); `OANOCACHE=1` for BSTR caching; "Do not point the symbol path on the analysis
computer to a symbol server" for diffs made elsewhere
([Analyze UMDH logs](https://learn.microsoft.com/en-us/windows-hardware/drivers/debugger/analyze-umdh-logs));
`-d`, `-v`, `-l` when analysing logs ([Analyze UMDH logs](https://learn.microsoft.com/en-us/windows-hardware/drivers/debugger/analyze-umdh-logs)),
`-g` when capturing ([Analyze a running process](https://learn.microsoft.com/en-us/windows-hardware/drivers/debugger/analyze-a-running-process)).
Not found in the pages read [U]: the stack depth limit and the exact shape of `-g` output.

### What it should be (ADR 0069)

- **A series** (2 to 64 snapshots) as the unit; logs checked to belong together, split at a restart.
- **Call site** identity by default (culprit after collapsing allocator and system frames, plus 2 callers), with
  Function and Stack one click away; one frame classifier shared with the profiler's "just my code" (two copies today:
  `crates/debrief-profile/src/frames.rs:138`, `crates/debrief-dump/src/modules.rs:106`; rule 55).
- **Trend per call site**: robust slope (Theil–Sen on the top 2,000), classes *steady growth*, *growth then plateau*,
  *one step*, *fluctuating*, *shrinking*, *new*, *gone*; per hour only when times are better than *weak*.
- **Symbols by identity** only from the same process instance (a dump or manifest of it; a PE of the same path and size
  marked *inferred identity*), then ADR 0036's lookup, since UMDH's module list has no time stamp or GUID; never
  by name; the unnamed share is a finding.
- **Times with provenance** (user, file name through ADR 0066's reader, collector manifest, weak mtime).
- **Sizes per call site** (log2 histogram and top exact sizes).
- **A check report** in ADR 0068's shape: verdict (or `CannotTell`), confidence with reasons, evidence, Not checked with
  a next step, left out.
- **Context**: Private Bytes from a `.blg` or monitor capture of the same pid ("is it even the NT heap?"), .NET hint,
  Moment and dump links.
- **Report block, CSV export, citations; Pitwall explains the computed facts only** (rule 46), checked by the engine.
- Settings for every threshold (today's constants as defaults); a job with phases and Cancel; bounded memory with
  interned frames; a Parallelism and cost section; ER-27 log lines without frame names.

Size: **27 days** for all of it, **18** for the first useful cut (series, call sites, classes, honest findings, UI).
Recommended: the two reader fixes and the test generator in 1.2 (about 2 days, app only, landing with SI-11); the rest in 1.3 (see the
roadmap note).

## Part 2: every other function

Sources: two read-only code surveys of this tree plus spot checks; each row cites the entry point. Thresholds quoted
are constants in code unless said otherwise.

**Patterns that cut across the list** [S]:

- **Fixed thresholds**: every log analysis gets `AnomalyConfig::default()` and `CompareConfig::default()`
  (`crates/debrief-core/src/workspace/insights.rs:122,133`; `crates/debrief-store-duck/src/analysis.rs:883,904`); only
  profiling findings take thresholds per request (`crates/debrief-profile/src/api.rs:794`).
- **Some analyses cover a sample at scale**: on the DuckDB store (the default, ADR 0010) anomalies (`anomalies_sql`,
  `crates/debrief-store-duck/src/analysis.rs:849-879`) and field statistics (`:1131-1160`) cover every row, but
  patterns, exception groups, compare (`analysis.rs:902-903`), spans and the query language stop at
  `MAX_ANALYSED = 2_000_000` (`analysis.rs:31`; the in-memory workspace has the same cap, `insights.rs:21`) and Moment
  `MOMENT_EVENTS = 250_000`, each with a `Coverage` note. Against the 1 billion event target that is 0.2 %.
- **Evidence is thin**: first-seen samples (patterns, exceptions), no ids (Silence, Compare, Field statistics), one id
  in health findings.
- **The query language refuses** a query that matches more than 2,000,000 events (`crates/debrief-store-duck/src/qlang.rs:66-77`);
  known issues (`known_issues::evidence`) and playbook queries go through it, so at the 1B target they fail rather than
  answer.
- **Event ids are `u32` on most DTOs**: QL results (`crates/debrief-core/src/ql/mod.rs:59`), anomaly and outlier
  `sample_ids` (`analysis/anomaly.rs:85`, `analysis/outliers.rs:72`), Moment's `record_id`, the store's anomaly ids
  (`crates/debrief-store-duck/src/anomalies.rs:122`); `sample_id()` saturates to `u32::MAX` "until 6c widens them"
  (`anomaly.rs:89`; promised by ADR 0014, never done). Store ids are never-reused `u64` blocks
  (`crates/debrief-batch/src/ids.rs:9`), so a 1B-event store with re-imports is within a few times of saturation.
- **Three event-reference types**: AI citations `Record { id: u32 }` (`crates/debrief-ai/src/assistant/citations.rs:11`),
  report `Events { ids: Vec<u32> }` (`crates/debrief-core/src/report/mod.rs:108`), note anchors `event_id: u64` plus time,
  source and excerpt (`crates/debrief-notes/src/anchor.rs`); the store's `EventId` is `u64` (`crates/debrief-batch/src/ids.rs:9`),
  and `EventsRequest` drops ids that no longer exist without saying so (`crates/debrief-core/src/ports/dto.rs:87`).
  This is the foundation gap behind citations (roadmap note, layer E).
- **Two windows only** for comparison; `patterns::compare` (`crates/debrief-core/src/patterns.rs:662`) is called only from
  tests.
- **No export** from analysis views; only query results (`app/src/app/analysis/query-results-view.ts`), reports and
  config diffs export.
- **No time in the query language** (no `@timestamp`, no `bin()`), so time bucketing needs the SQL console.

### Analyses

| # | Function | Today (entry point) | Rating, reason | Top enhancements | Size | Release | In plan |
|---|---|---|---|---|---|---|---|
| A1 | Anomalies | Five detectors over ~200 buckets: error spike (robust z ≥ 4, ≥ 5), silence (≥ max(30 s, 10 × median gap)), crash (≤ 5 s of end, restart within 120 s), slow outlier (global p99 and ≥ 3 × median), rate change (≥ 5× vs 10 buckets); ≤ 5 sample ids (`crates/debrief-core/src/analysis/anomaly.rs:355-632`) | **basic**: scores of different kinds are ranked together; p99 is global, not per operation; rate only for the whole stream; thresholds fixed | Comparable severity score per kind; per-operation latency baselines; rate change per instance and per pattern; thresholds as Settings; ids for silence (last event before, first after) | 4 | 1.3 | no |
| A2 | Memory growth | Least squares per series, ≥ 6 points, ≥ 10 min, R² ≥ 0.8, ≥ 50 MB and ≥ 20 % (`analysis/trend.rs:85,191`) | **basic**: a restart or a step hides a leak; field names hard-coded | Share UMDH's trend classifier (ADR 0069 D3): segments between restarts, plateau and step classes; field list as data | 1.5 | 1.3 | no |
| A3 | Window compare | TVD on fields ≤ 50 values (≥ 0.1), new/gone values (≥ 3), median/p95 change ≥ 25 %, top 10 (`analysis/compare.rs:319`) | **basic**: no significance, no message or pattern diff, no evidence ids, high-cardinality fields skipped silently | Wire `patterns::compare` in (new, gone, spiking templates); significance (G-test like outliers); evidence query per row; say what was skipped (ADR 0065 `Bounded`) | 3 | 1.3 | no |
| A4 | Field statistics | Count, missing, distinct (≤ 10,000), top 20, percentiles and histogram (`analysis/field_stats.rs:58`) | **basic**: no ids, no over-time view | Per-bucket percentiles over time; click a bar to filter; split by instance | 1.5 | later | no |
| A5 | Error outliers | G-test, lift ≥ 1.5, support ≥ 3, ≤ 60 fields, drill-down query and ≤ 5 ids (`analysis/outliers.rs:201`) | **useful**: the one analysis with statistics and evidence | Benjamini–Hochberg over the up to 6,000 tests; pairs of field values; time-scoped subset presets | 0.5 (BH), 2 (pairs) | 1.2 (BH, tiny) / 1.3 | no |
| A6 | Moment | Levels, per-instance counts, top 3 patterns, users, ≤ 200 activity rows (first come), metrics vs the previous equal window (`analysis/moment.rs:42`) | **useful** as a starting point; activity list is not ranked; baseline is one window | Rank activity (errors, first-of-kind, rare templates); baseline choice (same time yesterday, before a marker); add dumps, captures, video frames and notes in the window | 2 | 1.3 | no |
| A7 | Health check findings | Anomalies by severity (≤ 100), exception groups ≥ 5, import problems (`analysis/moment.rs:348`) | **useful** as triage; one id per finding | Finding shape of the dump ADR (verdict, confidence, evidence, could not check) for every finding (roadmap layer I) | 2 | 1.2 (shape) | dump ADR |
| A8 | Traces and requests | Fixed field names (TraceId, SpanId, ParentSpanId), waterfall, longest requests ≤ 500 (`analysis/trace.rs:51-261`) | **basic** | Field names from the profile; group by operation with p50/p95; critical path; link to the thread/sequence view | 2.5 | 1.3 | thread view approved |
| A9 | Patterns | Drain (similarity 0.5, depth 4), ≤ 10,000 clusters, 24-bucket trend, placeholder values (`patterns.rs:206,412`; ADR 0046) | **useful** | Samples spread over time, not first five; pattern compare between windows (code exists); "new since" a marker; CSV export | 2 | 1.3 | no |
| A10 | Exception groups | Parse .NET/Java/Python/JS stacks, fingerprint by types and top 5 frames, ≤ 10 samples (`exceptions.rs:235,471,510`) | **useful** | Trend per group; first seen relative to a deploy or restart marker; samples spread over time and instances; link to dump stacks of the same exception | 1.5 | 1.3 | .NET dump stacks (link only) |
| A11 | Runs (spans) | Start/stop pairing from `[[span]]`, statuses, ≤ 1,000 runs per lane, inference of rules (`spans/`) | **useful** | Duration percentiles and distribution; compare durations across windows; too-long runs as findings | 1.5 | 1.3 | no |
| A12 | Clock skew | Median delta over shared ids, confidence, ±7 days (`skew/mod.rs:180,202`; ADR 0006, 0037) | **useful** for a constant offset | Linear drift; feed ADR 0066's provenance (a skew suggestion is time evidence) | 3 | later | ADR 0066 (provenance) |
| A13 | Related logs (screenshot) | Image time ladder, ±120 s window plus clue searches, hand-tuned weights (`related/mod.rs:293`, `rank.rs:295`) | **useful** when the vision model reads the screen | Weights from the planted-incident corpus (layer J), not by hand | 1 | later | no |
| A14 | Folder survey | Sampled (≤ 100,000 events, 60 s) proposals for instance key, extractors, lifecycle and noise (`survey/mod.rs:169`) | **useful** for set-up | Run inside ADR 0067's scan where it can, so the dialog proposes before the import | 2 | 1.3 | ADR 0067 (scan) |
| A15 | Timestamp issues | Flags > 5 % without time or none of the first 20; proposes a format from 20 samples (`timestamp_issue/mod.rs:36`, `suggest.rs:28`) | **basic** | Becomes part of ADR 0066's problems (Z7); the `+h:mm` and spaced-date shapes (approved, Checkpoint A) | in Z7 + 1.5 | 1.2 | yes |
| A16 | Known issues | Fenced blocks with a regex or query, count and ≤ 5 ids (`known_issues.rs:141,262,282`) | **basic**: the query has **no time window** (`known_issues.rs:266`, a plain `Query`), so an incident report counts matches outside its period | Scope to the report's or view's window; first/last time per match | 1 | 1.2 (tiny) | no |
| A17 | Incident summary | Facts, key moments (≤ 20, earliest first), top anomalies by raw score, exceptions, error patterns, noisy hosts, problems, known issues, notes; optional AI narrative with checked citations (`incident.rs:131`) | **useful** skeleton; ranking and baseline are weak; a failed section disappears silently | Moments ranked by importance; compare with a baseline window; a failed section says so (ADR 0065 spirit); blocks become citable report blocks | 2.5 | 1.2 (with report blocks) | report blocks, Pitwall drafting |
| A18 | Playbooks | Steps: manual, open view, filter, query (≤ 20 rows), count checks (`playbook/run.rs:69`) | **basic**: no time window or parameters, count checks only | Parameters (window, instance, id); checks on analysis results (a finding exists); run results as report blocks | 2 | 1.3 | no |
| A19 | Name learning | Normalise, segment, group spellings, ranked match ways, optional local aliases (`names/`) | **useful** search aid | As planned (N2 to N8) | in plan | 1.2 | yes (N packages) |
| A20 | Configuration diff | Path-based tree diff, volatile keys ignored, folder snapshots by path, CSV/HTML export (`config/diff.rs:134`, `snapshot.rs:56`) | **useful** | Lists matched by a key field, not position; place a config change on the timeline (file times) so "errors after a change" can be asked | 2 | 1.3 | no |
| A21 | Charts and dashboards | Time-bucketed aggregates, ≤ 8 series, tiles (`chart.rs`, `dashboard.rs`) | **useful** | Overlay of two windows; thresholds and baselines on a chart; more than 8 series as small multiples | 2 | later | no |
| A22 | Profiling | Hotspots, trees, flame and diff flame, thread timeline, findings with evidence, heap snapshot diff/growth/dominators (`crates/debrief-profile/src/ops/findings.rs:188`, `heap/`) | **useful** | Thresholds exposed in Settings (they already exist per request); heap growth shares ADR 0069's trend classifier | 1.5 | 1.3 | no |
| A23 | Video | Scene-change frames (≤ 30 default, 120 max), time ladder with confidence, local vision descriptions, a lane per video (`crates/debrief-video/src/select.rs:172`, `align.rs:180`) | **useful** when the model is good | Align with ADR 0066 provenance; frames citable in reports | 1 | 1.3 | N6 done |
| A24 | Dump findings | Crash cause, culprit, overflow, heap corruption, lock waits, .NET present, missing symbols, handles, address space (`crates/debrief-dump/src/findings.rs:45`) | **basic** (the user's words) | The dump analysis ADR | that ADR | 1.2 part | yes (dump ADR, .NET stacks) |
| A25 | CLI `analyze` | anomalies, patterns, exceptions, issues, outliers (`crates/debrief-cli/src/args.rs:386`) | **basic**: always `Profile::default_profile()` (`crates/debrief-cli/src/run.rs:118`), so custom parsers and extractors do not apply headless | `--profile`; `compare`, `incident`, `umdh` subcommands later | 1 (`--profile`) | 1.2 (tiny) | no |
| A26 | AI tools | Read-only tools find_events, run_query, run_sql, patterns, exception_groups, overview, moment, get_events, field_stats, neighbours, find_related_logs (`crates/debrief-ai/src/assistant/tools.rs:557`) | **basic** for root cause: no tool for anomalies, outliers, compare, traces, runs, known issues, dumps or UMDH; citations by regex, unverified ones still shown | Tools over every engine analysis returning the shared Finding type; citations as typed references (layer E); with Pitwall drafting | 3 | 1.2 (with Pitwall drafting) | Pitwall drafting |
| A27 | Plugin analyses | Windows reboot and service annotators, crash/hang markers (`crates/debrief-plugin-windows/src/lib.rs:37-40,97-98`) | **useful**, narrow | A declared analysis contribution (returns Findings) once the plugin platform settles | 3 | later | no |

### Everything else

| # | Function | Today (entry point) | Rating, reason | Top enhancements | Size | Release | In plan |
|---|---|---|---|---|---|---|---|
| B1 | Import and parsers | 16 built-in parsers, per-line format choice from the first 200 lines, continuation lines kept and counted (`crates/debrief-core/src/importers/lines.rs:62-86`, `text.rs:15-22`) | **useful** | Event-start detection for weekday-first and `dd Mon yyyy` lines [U: inferred from `looks_like_event_start`]; streaming per ADR 0064 | in SI + 1 | 1.2 | ADR 0064 |
| B2 | Time shapes | RFC 3339/2822, numeric with day-first detection, English months, AM/PM, Japanese dates, epochs 2000–2100, named clocks; zones `Z\|UTC\|GMT\|±hh[:mm]` only (`crates/debrief-plugin-api/src/time.rs:121,461-478`) | **basic**: `+9:00`, `UTC+09:00`, zone names, spaced dates, `2026-Oct-14`, non-English months, dotted times fail; two-digit years always 20xx | `+h:mm` and spaced dates at Checkpoint A (approved); zone names through ADR 0066's `jiff` table; the rest as data | 1.5 + 2 | 1.2 / 1.3 | yes (two shapes), ADR 0066 |
| B3 | Encodings | BOM, then a statistical guess over 64 KiB; *Encoding…* re-read (`ingest/encoding.rs`) | **useful** | Mixed-encoding files [U] | 1 | later | no |
| B4 | Archives, bundles, paste | zip/gzip (1 GiB entry, 4 GiB archive, 65,536 entries), `.debriefz`, paste ≤ 8 MiB (`archive.rs:12-18`, `bundle.rs:19-27`, `paste.rs`) | **useful** | 7z, password zips [U]; members through `paddock-archive` (SI-4, SI-5) | in SI | 1.2 | SI-4, SI-5 |
| B5 | Folder open | Imports everything at once, no choice (ADR 0067 "What exists today") | **not useful** for large bundles today | ADR 0067 (scan, judge, tree grid, selection by folder) | 39.75 | 1.2 | yes |
| B6 | Watched folders and time-range import | Debounced, 4 GiB per file, 50,000 entries; range in UTC; files with mtime older than `since − 26 h` skipped (`watch.rs:14-31`, `ingest/window.rs:9`) | **useful** | Local and relative times ("last 2 h"), the zone from ADR 0066; say how many files the mtime rule skipped | 1 | 1.2 (tiny, with Z) | ADR 0066 |
| B7 | Explorer | Sources per folder with counts, badges, problems, menu (`app/src/app/shell/sidebar.ts:64-172`) | **useful** | First/last time per source (coverage gaps); "only this source"; group by host | 1.5 | 1.3 (after M3a/M3b move it) | M3a, M3b |
| B8 | Query language | Filters and stages `where stats sort head fields`, pseudo fields, cancel (`ql/parser.rs:452-527`) | **basic**: no time in the language, no `bin()`, no `top`/`rare`/`dedup`, no sequence operator | `@timestamp`, relative times, `stats … by bin(5m)`, `top`, `dedup`; later "A then B within N s" | 4 + 4 | 1.3 / later | no |
| B9 | Find, search everything, full text | Name-aware find (AI), search everything (2 s budget), Tantivy over message, exception and template (`find/mod.rs`, `search_all/mod.rs`, `crates/debrief-store-duck/src/fts.rs`) | **useful** | Phrase, OR and field values in plain search [U]; ADR 0062 makes it ready during import | in IX + 2 | 1.2 / 1.3 | IX, SR |
| B10 | SQL console | Read-only DuckDB, 10,000 rows, 30 s, 128 MB, ids kept (`ports/console.rs`) | **useful** for experts | Result as report block (layer H) | 0.5 | 1.2 (with report blocks) | report blocks |
| B11 | Timeline | Lanes per instance key, markers, runs, annotations, minimap, ≤ 500 lanes (`workspace/timeline.rs`) | **useful** | Thread/sequence lanes (approved); arrows for correlated events | in plan | 1.2 | 2.4, 3.3, 4.1, thread view |
| B12 | Filters, lenses | Levels, chips, facets, lenses (no time or instances), filter sets (`lens.rs`) | **useful** | Lens with a time window and instances, so "the incident view" can be saved | 1 | 1.3 | no |
| B13 | Inspector | Properties, exception, ±2 min, same correlation id, original lines (`workspace/context.rs`) | **useful** | Correlation chained over several ids; "all sources at this moment" inline | 2 | 1.3 | thread view |
| B14 | Live tail and alerts | Detectors and `[[live.rule]]`, mutes, cooldowns (`workspace/tail/`) | **useful** locally | Sequence rules; alert history into a report; outbound hook (network, opt-in) | 3 | later | no |
| B15 | Notes | Markdown anchored to one event, range, source, marker, chart point or image (`crates/debrief-notes/src/anchor.rs`) | **useful** | Several anchors per note ("A then B"); save an AI answer as a note; the event anchor becomes the shared evidence reference (layer E) | 2 | 1.2 (anchor type) / 1.3 | citations |
| B16 | Reports and exports | 7 block types, HTML and PDF, redaction; events by `u32` id; text links only http/mailto (`report/mod.rs:101-160`, `report/markdown.rs`) | **basic** for an investigator's write-up: no blocks for analysis results, no inline event citations | Typed blocks for every analysis result (Finding, Patterns, Outliers, Compare, Runs, Dump, UMDH, SQL) rendered by the engine; inline citations | in plan | 1.2 | report blocks, citations |
| B17 | Projects, sessions | `.debriefz` with store, notes, reports, jobs; autosave and restore (`project.rs`) | **useful** | Newer files refused (by design before v1) | - | - | - |
| B18 | AI assistant | Tools above, 6 rounds, outputs clipped to about a sixth of the context, citations by regex (`assistant/tools.rs`, `citations.rs`) | **basic** for root cause | See A26; drafting a report from findings (Pitwall drafting, approved) | in plan | 1.2 | Pitwall drafting |
| B19 | AI browser and research | Real Chromium, modes Off/Read along/Agent, cited research (`crates/debrief-ai/src/browser/`) | **useful** | Link a web source to the event or block that prompted it | 1 | later | no |
| B20 | Jobs | Queues per kind, cancel, retry, kept in the project (`app/src-tauri/src/jobs/`) | **useful** | Idempotency (J1); analyses over the whole store as jobs (layer F) | in plan | 1.2 | J1 |
| B21 | Settings | Application, data, AI, plugins, diagnostics (`app/src/app/settings/settings-model.ts`) | **useful** | An Analysis section with the thresholds (A1, A3, UMDH) | 1 | 1.3 | M2c |
| B22 | CLI | `import query analyze watch report` (`crates/debrief-cli/src/args.rs`) | **basic**: default profile only | `--profile` (A25); `export`, `sql` | 1 + 2 | 1.2 / 1.3 | no |
| B23 | Agent | cron, interval and trigger schedules, bundle, monitor, perfmon; no network (`crates/debrief-agent/src/`) | **useful** | Write the snapshot manifest UMDH times can rest on (ADR 0069 D5); optional UMDH action | 1 | 1.3 | no |
| B24 | Collector, monitor, perfmon | The bundle collector applies 7 of the core's 13 secret detectors with its own `[REDACTED]` mask (`crates/debrief-collect/src/redact.rs:10-16`, the same `debrief_core::redact` mechanism) | **useful**; a bundle and a redacted share mask different things | Choose the collector's detector set from the same Setting as the share, or say in the bundle which were applied | 1 | 1.2 (tiny) | no |
| B25 | Plugins | Signed packages, permissions, health, quarantine; importers listed in the survey | **useful** | As planned (PH1 to PH4, 4.4) | in plan | 1.2 | yes |
| B26 | Profiles and pipelines | TOML pipeline, 16 step kinds, markers, 20 versions (`profile.rs`, `pipeline.rs:703-1045`) | **useful** | As is | - | - | - |
| B27 | Home and dashboards | Tiles, level history, health check (`home.rs`, `tiles.rs`) | **useful** | Dashboard as one report block [U] | 1 | later | no |

### Top 10 by value for effort

Value is what an investigator gains on a typical case; effort is the size above. Ranked after the foundation work of
the roadmap note (none of these is allowed to compete with the streaming and folder-open chain, the user's first step).

| Rank | Enhancement | Rows | Size | Release | In plan |
|---|---|---|---|---|---|
| 1 | UMDH reader fixes (aggregated entries, unindented frames) and the series test generator, landing with SI-11 | Part 1 | 2 | 1.2 | no (new) |
| 2 | `+h:mm` and spaced-date time shapes | B2, A15 | 1.5 | 1.2 | **yes** (approved, Checkpoint A) |
| 3 | Known issues and playbook queries scoped to the window | A16, A18 | 1 | 1.2 | no |
| 4 | One shared Finding type (verdict, confidence, evidence, could not check) for health, dumps, UMDH, profiles | A7, A24, Part 1 | 2 | 1.2 | dump ADR (shape) |
| 5 | One evidence reference for notes, reports and AI citations, with "moved/gone" | B15, B16, A26 | 3 (+2 ADR) | 1.2 | citations (approved; this is its foundation) |
| 6 | Typed report blocks for analysis results, engine-rendered, citable | B16, A17, B10 | 5 | 1.2 | **yes** (report blocks) |
| 7 | CLI `--profile`; collector's detector set from the share's Setting | A25, B24 | 2 | 1.2 | no |
| 8 | Analysis thresholds as Settings (anomalies, compare, UMDH, profiling) | A1, A3, A22, B21 | 2 | 1.3 | no |
| 9 | UMDH series, first cut (ADR 0069 U0–U4, U6, U9) | Part 1 | 16 | 1.3 | no |
| 10 | Compare: pattern compare wired in, significance, evidence per row | A3, A9 | 3 | 1.3 | no |

Next in line: time in the query language (B8, 4), anomaly scores and per-operation baselines (A1, 4), AI tools over
every analysis (A26, 3, with Pitwall drafting), Moment ranking and baselines (A6, 2), exception trends (A10, 1.5),
outliers' Benjamini–Hochberg (A5, 0.5, tiny).

### Size

| Group | Raw days [J] |
|---|---|
| UMDH, all of ADR 0069 | 27 |
| Analyses A1–A27, enhancements not already in the plan | about 49 |
| Everything else B1–B27, not already in the plan | about 33 |
| **Total of new usefulness work** | **about 109** |
| Of which proposed for 1.2 (ranks 1, 3, 4, 5, 7, BH, B6, B10; ranks 2 and 6 are already approved) | about 14 |

Items already in the plan (ADR 0064 to 0067, N packages, the timeline and lists, the approved items) are not counted
here; the roadmap note prices the approved items that have no size yet.
