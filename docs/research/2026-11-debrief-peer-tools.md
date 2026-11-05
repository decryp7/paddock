# Debrief 1.2: how peer tools stay fast and small, and what to copy

Date: 2026-11-05. Read-only: docs, source and write-ups; nothing built or run. Debrief read at `origin/main`
`87c9b8449`, the plan at
`origin/docs/debrief-1-2-release-scope` `74e0cba23`, plan rows and the parse profile at
`origin/debrief/search-chain-rebase`
`e6f0d1de2`. Peer source was read at each default branch on 2026-11-05.

Marks: **[S]** read in source or the project's own docs. **[V]** vendor's claim, not reproduced. **[2]** secondary, not
verified.
**[calc]** my arithmetic from a format or field widths, not a measurement. **J** is my judgement. No cost or effect
below is measured.

**Answer.** Peers split into two families. Viewers (klogg, lnav, Procmon, PerfView) keep the data on disk and hold a
very small
index per line or event; analysers (Perfetto, DevTools) hold compact columns away from the UI thread. The servers (Seq,
Loki,
VictoriaLogs) skip blocks by coarse indexes, a cost model only partly relevant here. The desktop tools answer a screen
with a
viewport-sized range, and none of those I read rebuilds the full list of matches for each page. Debrief's store already
has this shape. Its two
real gaps are the ones the audit found: filtered results have no compact, reusable form, and profiles and heaps sit in
the UI's
process. Adopt a result bitmap first, then move captures out.

## 1. Peers

**klogg and glogg** (GPL-3, ideas only; glogg's last commit is 2018-08-13, klogg [forked it in
2016](https://github.com/variar/klogg/blob/master/README.md))
- Hold: the file stays on disk. Line ends are [delta-packed with streamvbyte in blocks of
  128](https://github.com/variar/klogg/blob/master/src/logdata/src/compressedlinestorage.cpp), with constant-time
  lookup. Search hits are a [`Roaring64Map` of line
  numbers](https://github.com/variar/klogg/blob/master/src/logdata/include/logfiltereddataworker.h). [S]
- **No mmap**, contrary to the usual description: [`QFile` reads in 5 MiB
  blocks](https://github.com/variar/klogg/blob/master/src/logdata/src/logdataworker.cpp), through a TBB graph. [S]
- Screen: `getLinesRaw(first, n)` reads just that byte range; the filtered view maps an index to a line number over the
  bitmap and a cache. [S]
- Bound: a `limiter_node` caps blocks in flight; search runs in chunks on a capped pool. [S]
- Scale: "10+ Gb is not a problem", no published memory figure. [V]
- Regret: too-long lines clear the index and show a critical message. PCRE was replaced by Hyperscan, then [a private
  fork of it](https://github.com/variar/klogg/blob/master/CHANGELOG.md). [S]

**lnav** (BSD-2)
- Hold: [`logline` is exactly 16 bytes](https://github.com/tstack/lnav/blob/master/src/log_format_fwd.hh) (time, 42-bit
  offset, level, flags, filter bits; a `static_assert` holds it there); a vector of line references per view. [S] That
  is 896 MB at 56M lines. [calc]
- Screen: lines are read and parsed on demand. SQL is SQLite [virtual
  tables](https://github.com/tstack/lnav/blob/master/src/log_vtab_impl.hh) with time, level and bloom-set constraints
  pushed down. [S]
- Bound: per-line cost only; `Index Memory` and `Line Buffer` are [reported separately to the
  user](https://github.com/tstack/lnav/blob/master/NEWS.md). [S]
- Scale: [3.3 GB, over 10M lines](https://docs.lnav.org/en/latest/performance.html) (v0.15 docs); the numbers are in a
  chart I could not read. [V]
- Regret: [filtering blocks the UI](https://docs.lnav.org/en/latest/performance.html). NEWS records a query that built a
  whole column index when a time range would do; it now scans the range. [S]

**Seq** (closed source)
- Hold: a Rust event store. [Signal indexes keep one bit per signal per disk
  page](https://datalust.co/blog/signal-indexing-beta), "at most a few megabytes" per GB (2018-10-16); [active signals
  intersect](https://datalust.co/docs/indexing). [S] Seven-day extents and a RAM cache of one-hour segments that drops
  the oldest: only in the Seq 5 docs as seen through a search index, the page now redirects. [2]
- Screen: signals narrow the pages, then events are read and filtered.
- Bound: I found no published figure in the current docs.
- Scale: `count(*)` on 1.5M events, disk only, laptop: no signal 15.4 s, Warnings 3.6 s, Errors 0.5 s, Exceptions 0.18
  s. [V]
- Regret: [large systems "often fall back to slow disk-backed
  searches"](https://datalust.co/blog/native-storage-in-a-new-seq-5-preview) (2018-08-21, why the store was rewritten).
  Old extents are never indexed. Indexes [cost I/O and CPU, and only help when
  selective](https://datalust.co/docs/indexing). [S]

**Grafana Loki** (AGPL-3, server)
- Hold: [compressed chunks per label set, a TSDB index of labels
  only](https://grafana.com/docs/loki/latest/get-started/architecture/), both in object storage. [S]
- Screen: narrow by labels, then fetch and grep chunks.
- Bound: per-tenant limits, a cluster concern.
- Regret: ["Loki was not designed or built to support high cardinality label
  values"](https://grafana.com/docs/loki/latest/get-started/labels/cardinality/). [Bloom
  acceleration](https://grafana.com/docs/loki/latest/operations/bloom-filters/) is experimental, "only pays off at large
  scale", aimed above 75 TB a month. [S] Row-based chunks are being replaced by [columnar "data
  objects"](https://techstrong.it/features/loki-pairs-with-kafka-and-smarter-storage-to-help-calm-cost-and-scaling-challenges/),
  "up to 20x less scanned", from press coverage. [2][V]

**VictoriaLogs** (Apache-2.0, server)
- Hold: column blocks of [at most 2 MiB uncompressed, const columns and dictionaries of up to 8 values in the
  header](https://github.com/VictoriaMetrics/VictoriaLogs/blob/master/lib/logstorage/consts.go); [bloom filters per
  block, 16 bits per token, 6
  hashes](https://github.com/VictoriaMetrics/VictoriaLogs/blob/master/lib/logstorage/bloomfilter.go). No inverted index
  is named; [the FAQ credits blooms, columns, stream grouping, a sparse time index and all
  cores](https://docs.victoriametrics.com/victorialogs/faq/). [S]
- Scale: ["up to 30x less RAM and up to 15x less disk"](https://docs.victoriametrics.com/victorialogs/) than
  Elasticsearch and Loki. [V]
- Regret: [2024-10-16: every query parsed all fields' metadata per
  block](https://github.com/VictoriaMetrics/VictoriaLogs/commit/202eb429a7d0022def274d3ff905c8c4012bbbca), fixed with a
  per-field offset index (10x on wide events, [V]). [2025-02-19: 256 files per part raised RAM too
  much](https://github.com/VictoriaMetrics/VictoriaLogs/commit/a50ab10998), reverted to 128. Streams must [never hold
  high-cardinality fields](https://docs.victoriametrics.com/victorialogs/keyconcepts/). [S]

**Perfetto** (Apache-2.0)
- Hold: parses everything once into a [columnar `Dataframe`, concurrent reads after
  `Finalize`](https://github.com/google/perfetto/blob/main/src/trace_processor/core/dataframe/dataframe.h). In memory
  only. [S]
- Screen: the UI sends SQL per track.
  [`BufferedBounds`](https://github.com/google/perfetto/blob/main/ui/src/components/tracks/buffered_bounds.ts) fetches
  3x the visible span, at a **power-of-two resolution, quantised so cache keys repeat**. Tracks query [mipmap tables
  built once per source](https://github.com/google/perfetto/blob/main/ui/src/components/tracks/slice_track.ts); counters
  use [2 buckets per pixel](https://github.com/google/perfetto/blob/main/ui/src/components/tracks/counter_track.ts) over
  an [implicit segment
  forest](https://github.com/google/perfetto/blob/main/src/trace_processor/plugins/counter_mipmap_operator/counter_mipmap_operator.cc).
  [S]
- Bound: none inside; the [browser's roughly 2 GB, with 2 to 4x inflation over the
  file](https://github.com/google/perfetto/blob/main/docs/visualization/large-traces.md); past that, a native local
  server. [S]
- Regret: multi-instance is documented as "a temporary workaround". [A parsed archive skips re-parsing but is
  version-coupled](https://github.com/google/perfetto/blob/main/docs/visualization/large-traces.md). [S]

**PerfView and TraceEvent** (MIT)
- Hold: [ETL converted once into
  ETLX](https://github.com/microsoft/perfview/blob/main/documentation/TraceEvent/TraceEventProgrammersGuide.md) on disk,
  because ETL is "tuned for event logging, not lookup"; [a page table of one entry per 1024
  events](https://github.com/microsoft/perfview/blob/main/src/TraceEvent/TraceLog.cs) gives random access by index or
  time; small integer indexes allow side arrays. [S]
- Screen: seek to a page and enumerate either way. Stack views aggregate a chosen time range.
- Bound: optional `/MaxEventCount` truncation with `/SkipMSec` for the next window. [S]
- Regret: the [users' guide](https://github.com/microsoft/perfview/blob/main/src/PerfView/SupportFiles/UsersGuide.htm)
  samples heaps by default at 250K objects: above 1M the viewer lags, above 10M is "VERY frustrating", with "a good
  chance" of running out of memory. [S]

**Windows Performance Analyzer** (closed source)
- [The product page](https://learn.microsoft.com/en-us/windows-hardware/test/wpt/windows-performance-analyzer) says
  nothing on scale; in-memory layout is unknown. Its [plug-in
  SDK](https://github.com/microsoft/microsoft-performance-toolkit-sdk/blob/main/documentation/Architecture/The-Data-Processing-Pipeline.md)
  asks parsers for **one pass**, with cookers consuming events and exposing outputs. [S] That is the extension model,
  not proof of the core.
- Regret: Dawson (2015) [strips Chrome's symbols to avoid "a twenty five minute delay when WPA loads the
  symbols"](https://opensource.googleblog.com/2015/04/uiforetw-windows-profiling-made-easier.html). [S]

**Process Monitor** (closed source)
- Hold: events go to virtual memory by default, or to backing files ([third-party
  guide](https://theitbros.com/process-monitor-procmon/)). [2] PML has [a table of offsets to every event, plus string
  and process tables](https://github.com/eronnen/procmon-parser/blob/master/docs/PML%20Format.md) (reverse-engineered).
  [2] Screen path unknown; the table suggests by event number. **J**
- Scale: ["tens of millions of captured events and
  gigabytes"](https://learn.microsoft.com/en-us/sysinternals/downloads/procmon), filters non-destructive, search
  cancellable. [V]
- Regret: [Drop Filtered Events did not drop](https://github.com/microsoft/windows-dev-performance/issues/36): an
  event's Result is unknown on arrival and "we don't go back to drop the event". 99 GB working set (2021), closed
  2024-07-24. [S]

**Chrome DevTools heap snapshots** (BSD-3)
- Hold: JSON parsed in a worker into [typed
  arrays](https://github.com/ChromeDevTools/devtools-frontend/blob/main/front_end/entrypoints/heap_snapshot_worker/HeapSnapshot.ts);
  a **second worker** builds retainers, dominators and retained sizes and transfers the buffers back. [S]
- Screen: providers answer `serializeItemsRange(begin, end)`, sorting only the prefix asked for. [S]
- Bound: I found no explicit cap; arrays split when an allocation fails. [S]
- Regret: [2024-04-04, a 227M-edge snapshot would not
  load](https://github.com/ChromeDevTools/devtools-frontend/commit/2fcf5d4cb3), and "the whole reason the reporter
  wanted to open the snapshot was to investigate that pathological behavior". Split arrays cost under 1 %. [S]

**Added: Tracy and the `regex` crate**
- Tracy: [all capture data in RAM until the capture
  ends](https://github.com/wolfpld/tracy/blob/master/manual/tracy.tex), about 1 GB per second at 27M zones in an extreme
  test [V]; its advice is more RAM or swap. Events too small to draw get a minimum size and a marked pattern. [S]
- `regex`: [fast engines cannot report capture offsets; the slower ones can](https://burntsushi.net/regex-internals/).
  This is why Debrief's profile looks as it does. [S]

## 2. Debrief against them

Debrief already has what most peers reached late: columnar zstd segments, a store worker separate from the UI, pages
capped at 5,000 rows (`ports/dto.rs:14`), an `ord(pos, id)` table so an unfiltered page is a range read (`pos.rs`), full
text in Tantivy. On the chain's `performance.md` (loaded machine) the large corpus imports in about 40 to 46 s warm into
a 4.95 GB store, and search is ready 5 to 10 times later, so the background builds are the long pole.

| Audit finding | Evidence, checked | Peers | Plan today |
|---|---|---|---|
| Profiles and heaps in the app process | `ProfilingSlot` in `app/src-tauri/src/profiling.rs:39` holds `Captures`, in-memory, `MAX_OPEN` 32 with no byte budget (`debrief-profile/src/store.rs:23,87`). Whole file read at `profiling.rs:196`. At the caps (`docs/profiling.md`: 20M objects, 100M references) the graph is 1.12 GB and the analysis at least 0.64 GB more [calc], per capture, against the unenforced 250 MB app budget | DevTools and Perfetto: never the UI thread or process. PerfView and Perfetto: cook once. The data is already columnar, so location is the issue, not layout | No package found (grep of both plans) |
| Fallback keeps all ids | `read.rs:538` `matching_ids` pushes every match into a `Vec<u64>`, and repeats it per page with no cache. All 56M matching is 448 MB [calc], above the 300 MB query budget. `fts.rs:86`: past 2M hits a word search also lands here, so the worst path takes the largest results | klogg: bitmap, reused | 4.2 ("stops collecting every id"), mechanism open |
| Deep paging with OFFSET | `read.rs:628`, and the same branch runs `count(*)` (`read.rs:602`) plus a second count for the anchor (`:618`) per page. CLI scan at 56M, "page at offset 10M": 1.6 to 1.9 s on DuckDB 2.0 ([table](2026-10-duckdb-2-and-large-data.md)); no app-level number | klogg, lnav: index into a held result | 4.2 lists paging, not this |
| Regex 38 % of parse time | `performance.md` on the chain, `:583`, 88 % of it `captures_at`. Tail split: 915 to 470 ns per line in a microbench; `is_match` 119 to 130 ns | klogg: one fast path, then Hyperscan | SR5 |
| Catalogue re-parses every event | H4 ([cpu-memory](../../apps/debrief/docs/research/2026-10-28-cpu-memory.md)): 652 CPU-s, 5 min, 911 MB at 7.6 GB. Chain profile: extraction 34 %, row fetch 26 %, allocation 14 % | lnav, Perfetto, PerfView: one pass; WPA SDK asks for it | P4, SR3, ADR 0062 |

## 3. Adopt, ranked

Costs are extra agent-days, **J**. Every effect needs an A/B block in `performance.md` (rule 48) before it is quoted;
designs for 1, 2 and 6 need a "Parallelism and cost" section (rule 51).

| # | Idea, from | Expected effect | Cost | Where |
|---|---|---|---|---|
| 1 | **Result bitmap over time positions** (klogg, lnav's filtered index). A filter's result becomes a `RoaringBitmap` of `pos` in the store worker, cached in the existing `KEEP` = 8 slots keyed by query and generation. Page = `select` a range; total = `len`; anchor = `rank`. Then `ord` gives ids, `rows_by_id` the rows | Replaces the id list, the OFFSET read and both counts after the first build. 448 MB becomes at most about 7 MB at 56M, any selectivity [calc: 855 containers, 8 KiB each], so at most about 56 MB for 8 searches. Second and deep pages near the cost of an unfiltered page | 3 to 5 | Inside **4.2**, on the chain's stable ids. `roaring` 0.12 is MIT or Apache-2.0 with `rank` and `select` [S] |
| 2 | **Captures out of the app process** (DevTools, Perfetto's native server). Stage a: byte budget and least-recently-used eviction in `Captures`, reopening from the path. Stage b: a worker behind the existing `ProfileStore` port, one per window or shared, with the importer reading a stream. Stage c, later: cook once to a versioned file, rebuilt on mismatch (ETLX, `perfetto` archive) | App stays within its 250 MB. A heap at the caps no longer risks the window. Reopen skips import and dominators (c) | a 1; b 4 to 6; c 3 to 4 | New **PR1, PR2, PR3**, lane 3, after 2.2. c may wait for 1.3 |
| 3 | **Memoise extraction within a chunk** (VictoriaLogs' const and small-dictionary columns: repeated values cost once) | Extraction is 34 % of the catalogue; P4 removes the row fetch. The saving depends on the duplicate ratio, unknown; invented samples will flatter it. Spike first | 1 | Inside **P4** |
| 4 | **Perfetto's request discipline for lanes**: power-of-two resolution, 3x padded range, quantised keys, 2 buckets per pixel where min and max matter; Tracy's marked minimum size for what is too small to draw | Plan 2.4 and 3.3 have buckets and a per-lane cache; quantising makes that cache hit while panning and zooming, and few bounds change per frame. `lane_units` and `histogram` are the coarse levels already | 1 to 2 | Inside **2.4** and **3.3** |
| 5 | **Report and enforce memory per structure** (lnav's rows; DevTools' reasoning). A test for the app budget, which [performance.md](../../apps/debrief/docs/performance.md) lists as "nothing (gap)"; bitmaps, captures and caches named in Diagnostics, with a positive control | Regressions of findings 1 and 2 become visible. ER-27, ER-33 | 1 to 1.5 | With **X5** and **PR1** |
| 6 | **Row-group signals** (Seq): one bit per row group per common filter (level, exception, then saved filters), written at import, turned into id ranges | Min and max on `level` skip little in a mixed group. Seq's numbers suggest large gains when selective [V]. Size: about 107 bytes per signal at 56M [calc]. **Measure first**: if bitmaps from idea 1 are enough for a session, drop this | 3 to 4 | **Y11**, gated. Old stores: rebuild, or Seq's regret |
| 7 | **Literal-delimited scanner for formats generated from templates** (klogg's fast path; `regex` internals). Where `text_format` knows the template, split on literals with `memchr`; the regex stays the fallback and the oracle in a differential test | Beyond the tail split, which itself still lacks an end-to-end measure. Perhaps 10 % less parse CPU on text logs **J**; `rx-bench` decides. Below 1 to 4 because import is not the long pole | 2 to 3 | Extend **SR5**. Measure the tail split first |
| 8 | **Growing results for scans** (lnav, klogg): fill the bitmap per segment and report count and progress; Cancel keeps nothing half-built | Long filters answer ER-18 instead of waiting silently. Needs idea 1 | 1 to 2 | With **4.2** |
| 9 | **PML by its offset table, with one pass** (PML format; WPA SDK; Procmon's regret). Read through the streaming path, never whole. Every "drop at import" filter is decided on arrival or says it cannot be | Keeps a new format off the known whole-file path. Offset details are reverse-engineered, so tests use invented files built from that spec | 0, a scope note | Inside **Y13**, written before it starts |

Also keep two reasons already in hand: VictoriaLogs' wide-event commit supports Y11's promoted fields over scanning
`extra`, and DevTools' 227M-edge story says limit messages (1.4, 3.6) must name the setting; a hard refusal hits exactly
the capture the user most needs. After PR2, raise Debrief's fixed heap caps to a memory budget in Settings.

Risk in idea 1: `ord` must be joined to the matches to obtain `pos`, and a hash join on 56M rows will spill at 192 MB.
Measure it. If that is too slow, write a `pos_of_id` array beside `ord` (224 MB at 56M [calc], about 4.5 % of that
store), or use a sort-free merge. During an import `ord` does not exist yet, so that window needs its own rule (cap and
say so, or a bitmap of ids). Fallback if none of this holds: keep ids as a sorted `u32` array at 4 bytes each and accept
OFFSET only on the exact path.

## 4. Reject

- **mmap as the way to read events.** klogg does not use it. Debrief reads through DuckDB, whose buffer pool has the
  limit; [Crotty et al., CIDR 2022](https://db.cs.cmu.edu/mmap-cidr2022/) lists the correctness and performance traps.
  Tantivy's mmap stays. A file Debrief writes itself, such as `pos_of_id`, is a separate decision.
- **A line index as the engine** (klogg, lnav). Great for one file, but no facets, analyses or lanes. A 16-byte in-RAM
  line is 896 MB at 56M against a 300 MB budget [calc]; Debrief's disk-based `ord` already does the job.
- **SQLite virtual tables** (lnav). DuckDB is the store; the idea worth keeping, pushing time down first, is already
  there.
- **Label-only index** (Loki). Its costs are object-store costs. It would fail on Debrief's unknown, user-supplied
  fields, the same cardinality trap Loki documents.
- **Blooms in place of Tantivy** (VictoriaLogs, Loki). Tempting, since the index is 1.52 GB of 4.95 GB, but blooms still
  read candidate blocks and word search needs positions and ranking; Loki itself says they pay off only at huge scale.
  Index build time is what hurts, and IX addresses it.
- **Hyperscan or Vectorscan.** [Capturing is ignored](https://intel.github.io/hyperscan/dev-reference/compilation.html),
  and captures are Debrief's cost. It also adds a C++ build on three platforms; klogg ended on its own fork.
- **Keyset paging as the fix for OFFSET.** Fine for Next, useless for a scrollbar that jumps; rank and select do both.
- **Loading events into an in-memory columnar store** (Perfetto). 2 to 4x the file breaks the budgets on 8 GB. Right for
  bounded captures only.
- **Sampled heaps by default** (PerfView). It protects a viewer Debrief does not have, and it blurs the retained sizes
  that findings rely on. A last resort under a stated budget, labelled.
- **Unbounded RAM and "buy more"** (Tracy), **pagefile as the store** (Procmon). Not for a tool with budgets.
- **CLP-style template storage** ([OSDI 2021](https://www.usenix.org/conference/osdi21/presentation/rodrigues)). A store
  rewrite for disk size, which is not 1.2's problem.

## 5. Unknown

- Not measured here: no peer or Debrief code was run. Bitmap sizes are format arithmetic; real overhead and build time
  are open.
- App-level cost of a deep filtered page and of a large capture in the app process: no record in `performance.md`.
- Duplicate ratio of `message` and exception text on real logs, which ideas 3 and 7 depend on. Synthetic corpora
  overstate it, and user logs may not be used.
- WPA and Procmon internals (closed source), and Seq's memory bounds today; lnav's memory figures are in a chart I could
  not read.
- Loki's data-object claims and Seq's RAM segment details are secondary.
