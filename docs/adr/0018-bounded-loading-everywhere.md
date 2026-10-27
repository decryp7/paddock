# 18. Bounded loading everywhere

- Status: Proposed (2026-10-27; the user reviews it before anything is built)
- Date: 2026-10-27
- Related: rule 10 (background work), rule 11 (progress), rule 14 (pipelines), rule 16 (performance budgets), rule 21
  (tunables are Settings), rule 40; ADR 0008 (long operations), ADR 0017 (viewport and selection, whose scrollbar
  markers use a bucketed query bounded here), ADR 0019 (limits explain themselves); Debrief ADRs 0042 (store port
  table), 0047 (time-window import), 0051 (streaming import), 0052 (stage pipelines)
- Feeds from: the DuckDB 2.0 and large-data research in progress on branch `docs/research-duckdb-2` (not pushed yet);
  its measurements set the default numbers below, which are placeholders until then.

## Context

Three reports from the same user, on the same data, are one pattern:

- **All timelines load at once.** The user wants to choose which timelines are shown, with a default of about 10 and a
  configurable limit.
- **Slowness with a 7.6 GB import.**
- **Memory blow-ups.**

The pattern: **the amount of work and memory scales with the data, not with what is on screen**, because several
paths have no limit, or a limit that is a hidden constant far above what the UI can show. Paths below are under
`apps/debrief/` unless they start at the repository root.

### Timelines

- A *lane* is one instance key (`crates/debrief-core/src/query.rs:331-365`); a file with many instance keys gives many
  lanes. The engine sorts the lanes and truncates them at a hidden `MAX_LANES = 500`
  (`crates/debrief-store-duck/src/read.rs:30,949-959`; the in-memory engine has its own copy in
  `crates/debrief-core/src/workspace/timeline.rs:9`). There is no default number of lanes and no Setting.
- The UI asks for every lane (`app/src/app/core/workspace/timeline-data.store.ts:41-54`, the lane filter `groups` is
  empty = all, `timeline-source.ts:427`) and renders every one as DOM with an SVG density chart and a button per
  marker (`app/src/app/timeline/timeline-lanes.ts:41-120`); nothing is virtualized, and `buildLaneVms` recomputes every
  lane on each change (`timeline-panel.ts:233-248`).
- Up to 12 extra timeline panels (`MAX_TIMELINES`, `timeline/timeline-config.ts:12`) are restored from
  `localStorage` (`debrief.timelines`) and each **queries on start**, visible or not: one `queryTimeline` plus
  `profiling_spans` and `dumps_markers` per panel, plus annotations, runs (an engine scan of every timed event,
  `crates/debrief-store-duck/src/analysis.rs:1071`) and the minimap.

### Engine queries (33 `Store` port methods, `crates/debrief-core/src/ports/mod.rs`)

**23 are bounded** (page: default 500, max 5 000; facet 1-1 000; timeline buckets 10-2 000; search max 10 000; SQL
10 000 rows / 32 MB / 30 s; and so on). **10 are not:**

| Method | What grows |
|---|---|
| `summary.sources` | every source; the sidebar renders them all (`app/src/app/shell/sidebar.ts:78,113`) |
| `run_query` | QL `Events{ids}` up to 2 M ids; table rows uncapped (`crates/debrief-core/src/ql/exec.rs:225`) |
| `exception_groups` | number of groups |
| `anomalies` | number of results |
| `traces` | trace list and waterfall |
| `problems` | rows per source x kind |
| `parsers` | one per source |
| `tail_files` | one per tailed file |
| `scan`, `export` | unbounded by design (callback, file stream); they stay so, as jobs |

Where a cap exists the caller usually cannot choose it (lanes, patterns, correlate, requests, moment, exceptions,
anomalies), the UI is not told that it was hit, and none of the caps is a Setting. The Tauri commands pass through
(`app/src-tauri/src/workspace_commands.rs:366-399`, `insights.rs`, `store_pane.rs`) and add no limits. Inside bounded
answers, memory still grows with the data: the page fallback collects every matching id into a `Vec<u64>`
(`crates/debrief-store-duck/src/read.rs:537-550`).

### Import

Text and JSON-lines stream in bounded memory (`crates/debrief-core/src/ingest/source.rs:331-342`, segments of 131 072
rows). Three paths read the **whole file** into memory: `PlanKind::Binary` (`source.rs:323-327`, e.g. `.evtx`),
`PlanKind::Legacy` (`source.rs:347-351`, every importer other than text and JSON, e.g. CSV, then decoded into one
`String`), and JSON arrays and single objects (`ingest/plan.rs:29-32`). Archives expand in memory up to 1 GiB per entry
and 4 GiB in total (`crates/debrief-core/src/archive.rs:10-12`). The store worker has a 2 GiB cap
(`crates/debrief-store-client/src/client.rs:68`), so a 7.6 GB CSV, JSON array or binary file cannot import, and two
1 GB files in parallel can exceed it. Out-of-process importers copy the file again and collect every record before
returning (`crates/debrief-plugin-windows/src/evtx_import.rs:57-67`, `crates/debrief-plugin-host/src/remote.rs:145-170`).

### UI lists

Four Debrief lists are virtualized (events grid, facet popover, Home findings, config diff); at least 14 render every
item of an engine array with `@for` (timeline lanes, patterns, exceptions, anomalies, traces, outliers, QL results, SQL
console, problems, runs, sidebar files, fields, moment, compare). `PdTable` (`ui/projects/paddock-ui/table/src/table.ts`,
virtual above 100 rows) exists and Debrief does not use it.

### Tests

The nightly store benchmark (`.gitea/workflows/debrief-bench.yml`) uses a 10 M-event, 3.7 GB corpus with 20 instance
groups; the samples are 17 MB, no file over 5 MB. Nothing tests a single file larger than 2 GiB, a source with
hundreds of lanes, or the UI with a large result. Debrief has not adopted `crates/paddock-bench`.

## Decision

### The contract

1. **Every engine query that returns a collection has a limit or a page**, as a typed argument (`Limit`, `Page`,
   `Buckets`) with a documented default and a hard maximum, and every answer says whether it was cut
   (`truncated: Option<Truncated { shown, total_or_lower_bound, limit: LimitId }>`). A collection that must be
   complete is a job that streams (export, scan), never a reply. This applies to the `Store` ports, the Tauri commands
   and the shared `paddock-*` query surfaces (`paddock-view` inline rows already follow it).
2. **Every UI list or timeline is virtualized or capped.** A list renders only what is in view (`PdTable` or the
   shared virtual helper of ADR 0017), or shows at most its limit with "Showing N of M · Show more"; the limit hit is
   visible, never silent.
3. **Default limits are Settings** (rule 21), declared once in the engine's settings schema under a **Limits** group of
   Performance (lanes per timeline, rows per page, facet values, analysis results, search hits, groups per analysis),
   with min, max and unit; the hard maximums stay constants because they protect the process. A caller may ask for
   less than the Setting, never more than the hard maximum.
4. **Imports stream.** Every import plan reads through a bounded reader and hands records to the batcher in chunks:
   CSV and other legacy importers move to a streaming `Importer` method; JSON arrays use a streaming array reader;
   binary importers (evtx) read and return records in chunks (the IPC already chunks events at 4 MiB or 8 192 events)
   instead of one buffer and one `Vec`; archives extract to a temporary file, not memory. Memory per file in flight is
   bounded by a constant (proposed 64 MiB, measured by the research), so the worker cap divided by that bound sets the
   number of files in flight, not the thread count alone (rule 13: memory-heavy work is bounded by RAM).
5. **Work follows visibility.** A view that is not visible (a background tab, a collapsed panel, an auto-hidden tool)
   does not query; it marks itself stale and queries when shown. Restored timeline panels are created lazily.

### Timeline selection

6. **A timeline shows a chosen set of lanes, default 10.** The engine answers `queryTimeline` with the top N lanes by
   the panel's ordering (events in range by default; also errors, name, first seen) plus a lane *catalogue*: the
   total count and a paged, searchable list of the others (name, count, sparkline bucket counts at low resolution).
   N comes from the Setting **Timeline ▸ Lanes shown by default** (default 10, range 1-200; the hard maximum stays
   500).
7. **The user chooses.** The lane header has a **Lanes** picker (CDK menu with search, rule 17: in-app): check or
   uncheck lanes, pin lanes so they stay regardless of order, "Show top N", "Show all matching" (up to the hard
   maximum, with a warning above 100). Below the last lane a row says "12 more lanes · Choose…". The choice is part of
   the panel config, saved with the project and layout; a lane that no longer exists is dropped quietly.
8. **Lanes render virtualized** (only lanes in view are DOM; or one canvas per panel once profiling shows DOM is the
   bottleneck), and lane view models are computed per lane, keyed by lane id, so one lane's update does not rebuild
   the others.

### Where it lives

9. Shared (rule 39, 42): the typed `Limit`/`Page`/`Truncated` and the `bounded_query` test helper in a small part of
   `paddock-view` (or a new `paddock-query` if it does not fit there); the "Showing N of M" footer and the lane picker
   in `paddock-ui`; the large-fixture generator in `paddock-testkit`. Debrief keeps its query implementations and its
   lane semantics.

## The check

1. **Engine: every collection endpoint is bounded.** A test in each store crate enumerates the port methods (one
   table, kept next to `ports/mod.rs`, that the compiler checks: adding a method without a row fails to build) and,
   for each, generates data larger than its maximum (from the large-fixture generator) and asserts the answer has at
   most the maximum items and a `truncated` marker. `scan` and `export` are listed as streams and asserted to keep
   memory flat instead.
2. **Lint over the command layer** (conformance check, Rust): a `#[tauri::command]` or port method whose return type
   contains `Vec<_>`/`HashMap<_,_>` and whose arguments contain no `Limit`/`Page`/`Buckets` is an error unless it is
   listed in `boundedBy` of `paddock.conformance.json` with a reason ("one per source: bounded by the source cap",
   "settings sections: fixed set"). This keeps the 10 unbounded methods visible until they are fixed.
3. **Import memory test:** a generated 8 GiB sparse-ish CSV, JSON array and evtx-shaped binary (built on the fly in a
   temporary folder, never committed) import with peak worker memory under the per-file bound plus a fixed overhead;
   nightly, with the measured peak recorded by `paddock-bench` (rule 16) and the 7.6 GB case as a named budget.
4. **UI test with large fixtures:** the mock backend gains a "large" profile (500 lanes, 1 M events, 100 000 patterns,
   10 000 sources). A Playwright spec opens each data view and asserts (a) the number of rendered rows or lanes is at
   most the visible count plus overscan or the limit, (b) "Showing N of M" is shown when cut, (c) the timeline shows
   10 lanes and the picker lists the rest, (d) scrolling stays within the rule-16 frame budget. A unit test asserts
   that hidden panels issue no queries.
5. **Timeline regression first** (rule 5): with 60 lanes, the timeline must render 10 and offer the other 50.

## Consequences

- The time and memory a view needs depend on the screen and the Settings, not on the file; a 7.6 GB file imports in
  bounded memory, at the speed the research measures, instead of failing at the worker cap.
- Every hidden cap becomes visible to the user ("Showing 10 of 312 lanes", "Showing 10 000 of 2.4 M hits") and
  adjustable within its hard maximum.
- Importers change: the legacy (whole-string) importer contract and the binary importer contract gain streaming
  forms; existing importers move over one by one, with the whole-file form kept only behind a size limit with a clear
  message (ADR 0019) until they do.
- About 10 engine methods, 14 UI lists and the timeline change; the rest only gain the `truncated` marker.

## Migration plan

1. Failing tests: 60-lane timeline, a 3 GiB CSV import under the worker cap, an exceptions view with 50 000 groups.
2. Timeline lanes: Setting, top-N answer with catalogue, picker, virtualized lanes, lazy restored panels.
3. Typed `Limit`/`Page`/`Truncated` on the 10 unbounded methods; the command lint with the remaining entries listed.
4. Streaming import for CSV/legacy, JSON arrays and binary (evtx first, with ADR 0019's crash), archives to disk.
5. UI lists onto `PdTable` or "Showing N of M"; the large mock profile and its spec.
6. Defaults tuned from the `docs/research-duckdb-2` measurements (page size, lanes, DuckDB memory limit, files in
   flight), recorded in `paddock-bench` budgets.

## Alternatives considered

- **Raise the worker cap.** It moves the failure to the next larger file and to machines with less RAM.
- **Cap only in the UI.** The engine would still compute and send 500 lanes or 2 M ids; the cost is in the engine.
- **One global row limit.** Lanes, rows, groups and hits have different costs and different useful defaults; one
  number would be too low for some and too high for others.
