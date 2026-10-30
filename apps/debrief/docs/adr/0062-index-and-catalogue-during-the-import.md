# 62. Stable event ids: the search index and the catalogue are built during the import

- Status: Proposed (2026-10-30; the user's decisions of 2026-10-31 are recorded below; Accepted after an independent
  review)
- Date: 2026-10-30, decisions 2026-10-31
- Related: ADR 0010 (persistent store), ADR 0014 (database-first engine; §2 step 4 already planned "index: Tantivy on
  its own thread, from the same batches"), ADR 0037 (live tail), ADR 0042 (port table), ADR 0051 (stable source ids),
  ADR 0052 (stage pipelines); Paddock rules 11, 13, 14, 48, 51 and 52; ER-18, ER-20; plan `docs/plans/debrief-1-2.md`
  packages P4 and 5.2

## Context

The user wants the import to do everything at once: each parsed batch goes, at the same time, to the Parquet writer,
the full-text indexer and the catalogue (names) aggregation. Today, on `debrief/duckdb-2` at 433565981:

- The import holds the store's write lock for its whole length (`lib.rs`, `import_keeping`), so the indexer
  (`fts.rs`, `step` takes `write_lock.try_lock()`) and the catalogue (`catalogue.rs`) start only when it ends.
- The indexer reads every segment back from Parquet (`fts.rs`, `SegmentBatches`), although the sink already has every
  batch in memory with its final ids and calls an `observe` hook with it (`writer.rs`, `SegmentSink::write`); the import
  passes `None` for that hook.
- The catalogue reads each source back through DuckDB, one source at a time on one thread (`catalogue.rs`, `build`).
- Time-order positions (`ord`) exist only after `pos::rebuild`, a window sort over every visible event at the end of the
  import (`pos.rs`).

Baseline, `large10` (56.2M invented events, 8.07 GB, 618 files), warm, load 5.2, release, M4 Pro
(`performance.md`, "Import of an 8 GB folder until search is ready"): import 39.7 s, search ready at 208.3 s,
catalogue at 376.4 s. The background builds, not the import, are the long pole. Sampling found: the index bound by
Tantivy's 2 writer threads (about 330k documents/s), the catalogue on one core for 3 to 6 minutes, `pos::rebuild` about
8 s of the import's end, and no overlap at all.

A first prototype (`wip/debrief-duckdb2-prototypes`, `gate.rs`, commit 5636776d4) let the index and catalogue steps run
inside an "append window" beside the import. The import became 4 to 5 times slower (235 s and 180 s against 46 s):
parsing still ended at about 42 s, then the import waited about 110 s with the CPU nearly idle for the steps inside the
window to finish before it could close it. The cause was not found. At that time the steps ran at macOS QoS `BACKGROUND`,
which keeps threads on the efficiency cores; 5bac7512c later found exactly that effect on the index writer and moved
lowered threads to `UTILITY` (rule 52). That is the likely cause, but **not verified**. Either way, that design made the
import wait for background work, which this ADR forbids.

### What the code already guarantees about ids (verified by reading)

- `EventId` is a `u64` (`debrief-batch/src/ids.rs`). The sink gives every segment a block of `SEGMENT_ROWS` (131,072)
  ids when it opens (`writer.rs`, `open_segment`); a batch gets consecutive ids from its segment's block in the parse
  thread, *before* it is written (`batch.assign_ids(EventId(w.next_id()))`). So a batch has its final ids at the moment
  it could be handed to another consumer.
- Ids are never reused: `meta.next_id` is persisted with every listing, and at once per reserved block when an observer
  is set (`writer.rs`, "listed ids are never reused, even after a crash").
- Ids never change: compaction rewrites rows with their ids (`compact.rs`, `rewrite` uses `write_existing`); a removed
  source's ids go with it.
- A segment's ids are one contiguous range `[first_id, end_id)` (the `segments` table), which is how `rows(ids)` and
  `texts(ids)` read only the segments that hold the ids (`pos.rs`, `files_for`).
- The index already stores the id, the source, the segment key and the time (`fts.rs`, `schema`: `id` FAST, `source`
  INDEXED, `seg` INDEXED, `ts` FAST) and already sorts hits by `ts` (`order_by_fast_field("ts")`). It never used
  positions (index `LAYOUT` 2 dropped them).
- Ids are not reproducible between two imports of the same files: which block a batch lands in depends on which parse
  thread wrote it. Within one batch the ids follow the file order. `pos` orders by `(coalesce(ts, MAX), source, line,
  id)`, so the id only breaks ties between events of the same source line, which come from one batch: page and
  position order are deterministic although the ids are not.
- Notes anchor an event by its id (`debrief-notes/src/anchor.rs`, `Anchor::Event`) with time, source and an excerpt
  shown when the id no longer resolves; a reload that reads a source again gives its events new ids.
  Re-anchoring such notes is out of scope here (decision 3 below).

So the stable id the user asked for exists. What is missing is everything around it: the index and the catalogue must
accept batches of sources that are not visible yet, hide them until they are, survive a crash or a cancel mid-way, and
never slow the import down.

## Decision

### 1. The id scheme: keep the block-allocated `EventId` as the one event identity

The event id stays what it is: a `u64` from per-segment blocks, assigned in the parse thread before the batch is
written, never reused, never changed. It is the primary key of an event in every layer (Parquet rows, `ord`, the index,
the catalogue's first and last ids, notes). No mapping table is added; there is no second id.

| Candidate | Unique for every importer | Known at parse time without a barrier | Fits `u64` | Keeps segment id ranges contiguous (`files_for`) | Verdict |
|---|---|---|---|---|---|
| **Block id (today)** | yes | yes (the sink's block) | yes | yes | **kept** |
| Source id + byte offset | no: binary and legacy importers write `byte_offset` 0 (`ingest/legacy.rs`, `row.rs`); one line can give several events | yes | no: 32-bit source + 32-bit offset caps files at 4 GiB (decompressed archives exceed it) | no: a segment holds batches of many sources | rejected |
| Source id + line or record number | no: several events per line (JSON arrays, records split by importers); record ordinals need every earlier chunk's count | lines yes (the pre-scan), record ordinals no | only with a narrow source field | no | rejected |
| Import batch sequence + index in batch | yes | needs one global counter per batch (a contention point the block avoids) | yes | no: batches of one sequence spread over segments | rejected: the block id is the same idea with fewer holes and no shared counter per batch |
| 64-bit packed (source:24, ordinal:40) | yes if the ordinal is per source | no: a per-source ordinal over parallel chunks needs the counts of every earlier chunk (a barrier) | yes | no | rejected |

**What the index stores per event (index `LAYOUT` 5):** `id` (FAST), `ts` (FAST, untimed as `i64::MAX`, as today),
`source` (INDEXED and FAST), `line` (FAST), `seg` (INDEXED, the segment key) and the new `gen` (INDEXED and FAST): the
import generation that wrote the event (the existing `meta.file_generation`, one per `SegmentSink`).

**From an id to a row or a position:** unchanged. Rows: `rows(ids)` and `texts(ids)` find the segments by their id
ranges and filter `READY`. Positions: `position_of(id)` reads `ord` and returns `None` until the event is positioned
(the port already says so). No query needs an id-to-position mapping beyond `ord`.

**Re-imports (reload).** A reload keeps source ids (ADR 0051) but reads the files again, so the events get new ids in a
new generation. Until a re-read source is ready, its previous rows and their index documents keep answering; when it is
ready, the store's catalog records the source's new generation in the same transaction that makes it ready
(`sources.gen`, a new column), so search switches from the old documents to the new ones at that instant (build new,
then swap: ER-18). The old generation's documents of that source are deleted afterwards (`IndexWriter::delete_query`,
in Tantivy 0.27, on `source = s AND gen < g`) and are invisible until then. Notes on events of a re-read source resolve to "no longer
resolves" with their excerpt, as today; matching them again by (source, line, excerpt) is out of scope: a later,
separate Debrief package (decision 3 below).

**Duplicates.** One file at two paths is two sources (ADR 0051 never guesses); a path already known in the import is
skipped (`known` in `import_keeping`). The store never stores the same event twice under one source, so the index needs
no de-duplication. Within the index, a document is never added twice for one segment: every path that indexes a segment
first deletes the segment's key (as `index_files` does today), and the tee obeys the invariant in section 3.

**Append to an existing store (live tail, ADR 0037).** Appended events get new ids in a new segment of the same source
and the source's current generation; the tee indexes them like an import's batches; `pos::append` extends positions
without a sort when they sort after the last positioned event, as today.

**Deleting a source.** As today: the catalog deletes the source (it stops being ready at once, so its documents are
filtered out at once), then the index deletes `source = s` and the catalogue its rows. Source ids are never reused
(the import persists `meta.next_source` before any index commit that names a new source, see section 3), so a crash
between the two deletes leaves documents that no live source owns; the open-time sweep removes them.

### 2. Time order without a global sort before search

The index answers in time order by itself. `pos` stays a store-side concern for the unfiltered event list and jumps.

- Search sorts by the stored fields, with the same order as `pos`: `(ts, source, line, id)`. Today only `ts` orders
  the hits, so equal times come back in an order the code does not define. A top-k collector over the four fast fields
  makes the order total and equal to `ord`'s, so a hit list and the event list agree. Tantivy 0.27 (the locked
  version) takes tuple sort keys and closures in `TopDocs::order_by` (`collector/sort_key/sort_key_computer.rs`, read,
  not yet tried); its cost against the single `ts` key is measured in prototype B.
- Search counts and returns only hits whose `(source, gen)` is live: the source is ready and `gen` is its current
  generation. The collector reads both from fast fields and checks a small table built per search from `sources`
  (thousands of entries at most). Not-ready sources, aborted sources and older generations are therefore invisible
  without having been deleted yet.
- Mid-import, a hit whose event has no position yet still opens: its row comes from `rows(ids)`; the event list scrolls
  to its time (`position_of` is `None`, the UI anchors by time) until positions exist. The UI keeps saying "Ordering
  events by time" for that phase, as now.
- `pos::rebuild` stays at the end of the import; the Parquet writer and `pos` are not touched by this ADR. Its cost (8
  s, spills, the 664fdf6d6 retry) is a separate item.

### 3. The pipeline

```
discover ─▶ dispatcher ─▶ parse pool (threads) ──▶ SegmentSink.write ── Parquet segment (always, in the parse thread)
                                                         │ observe(&batch) after the write, no lock held
                                                         ├─ try_send ─▶ index queue (bounded) ─▶ the index owner thread
                                                         └─ try_send ─▶ names queue (bounded) ─▶ catalogue workers
```

- **One owner per store artefact.** The index has one writer (Tantivy allows one); its owner is the existing
  `FtsService` thread, which during an import consumes the index queue and otherwise catches up from Parquet. The
  catalogue workers (prototype 71c18c57c) consume the names queue. During an import the sink is the only writer of the
  catalog database: it records index progress and catalogue rows in its own listing transactions. Nothing in this design
  takes the store's write lock beside the import, and the append window of `gate.rs` is not needed.
- **The import never waits for a consumer.** `observe` (already called after the segment write with the slot released)
  `try_send`s an `Arc` of the batch (Arrow arrays are shared, not copied). When a queue is full the batch is skipped:
  - index: its segment is marked *partial*; later batches of that segment skip too (nothing is gained by indexing a
    segment that will be indexed again from Parquet);
  - catalogue: its source is marked *partial*; its tally is dropped and the source is built after the import.
- **Catch-up.** A partial or skipped segment ends unmarked (`segments.indexed = FALSE`); the index owner indexes it
  from Parquet after the import with the existing pipeline, with parallel segment readers (prototype A). A partial source
  goes to the catalogue's `pending()` list as today.
- **The index's invariant.** A segment is marked `indexed` only after an index commit holds all its rows; a segment that
  is not marked has no committed documents except ones its key deletes. Before compaction may merge unmarked segments
  (which changes their file names, so their keys), the tee is drained and, in one index commit, the documents of every
  partial segment are deleted by key. On open, the same delete runs for every unmarked segment before any compaction.
- **Commit points.**
  1. Parquet: a segment is listed (with its sources made ready) every `LIST_EVERY` (1 s), as today.
  2. Index: the owner commits about every 10 s or 512 MB of input text, whichever is first, and at the end of the
     import (the 10 s cadence is the user's decision 1 below); after each commit it hands the sink the set of segments now complete in the index. The sink marks them
     `indexed` in its next listing transaction (or `UPDATE` for segments listed earlier). The reader reloads after each
     commit (`ReloadPolicy` as now).
  3. Ids and sources: before the owner commits documents, the catalog has persisted `meta.next_id` (already the case
     with an observer) and `meta.next_source` covering every id and source the commit names.
  4. Catalogue: a source's tally is merged when the source is finished; its rows are written in the listing
     transaction that makes it ready (or the next one), so a lookup sees its names when it sees its events, as now.
  5. End of import: drain the queues (bounded, so this wait is short), final index commit, last listing, compaction,
     `pos::rebuild`, checkpoint. Then the background catch-up of whatever was skipped.
- **Determinism of the catalogue.** A tally is capped (`MAX_NAMES`, `MAX_EVENTS_SCANNED`, `MAX_GUIDS`), so what enters
  it depends on the order of events. Chunks of one source arrive in any order, so the workers keep one tally per (source,
  chunk index) and merge them in chunk order, applying the caps during the merge; the result is identical for 1 or 16
  threads (differential test against the post-import build on `/samples`). Field values (today a SQL `GROUP BY` over
  `extra`) become per-source counts with the same caps, plus a new one on the number of keys per source.
- **Cancel.** The import's cancel stops the tee with the parse threads. Sources finished before the cancel stay
  (today's rule: what was read stays usable) and keep their documents and tallies; aborted sources are deleted from
  `sources`, so their documents are invisible at once and deleted in the drain commit; their tallies are dropped.
- **Crash.** Committed documents of unlisted segments or unready sources are invisible (not live). On open, a sweep
  deletes documents whose `(source, gen)` is not live and documents of unmarked segments (above); unmarked segments are
  then indexed again. Ids and source ids are never reused, so a stale document cannot point at another event.
- **What search shows mid-import.** Events of ready sources whose documents are in a committed index generation: at
  most one commit interval (about 10 s) behind the event list, plus whatever the tee skipped, which appears as the
  catch-up proceeds. `index_status` reports both (documents indexed, segments still to index), so "Search is still
  indexing N segments" stays true and live (rule 11).

### 4. Bounded resources and memory

The store worker's import budget is 768 MB (ADR 0014; measured peaks are already 905 to 979 MB physical footprint on the
2.42 GB corpus, `performance.md`, and plan package 4.3 owns cutting them). The tee gets a fixed share of that budget,
taken from the import memory setting, never added to it:

| Part | Bound | Default share |
|---|---|---|
| Index queue | batches and bytes of the Arrow buffers it keeps alive | 16 batches, 32 MB |
| Index writer heap | threads x heap per thread (Tantivy needs at least 15 MB per thread); during an import the threads are capped so parse + index + catalogue threads stay within cores minus one | 4 x 32 MB = 128 MB |
| Names queue | batches and bytes | 8 batches, 16 MB |
| Catalogue tallies in flight | bytes over all (source, chunk) tallies; at the cap the largest source's tally is dropped and the source built later | 64 MB |
| Total | | 240 MB |

When the import memory leaves less than this beside the parse pool (low memory mode, or a setting below the default),
the tee is off and the import behaves as today (the background builds after the import, with parallel readers). The
prototypes measure the physical footprint (`phys_footprint` on macOS, PSS on Linux), not RSS (rule 26).

### 5. Parallelism and cost (ER-20, rule 51)

| Stage | Kind | Threads (default, a Performance setting with Auto) | Role (rule 52) | Cheaper? |
|---|---|---|---|---|
| Parse pool | CPU | `min(cores - 1, 8)` (unchanged) | Bulk: `UTILITY` / nice 10 / `BELOW_NORMAL` | regex tail split (433565981), about 15 to 20% less parse CPU expected, not measured end to end |
| Parquet write | CPU, in the parse threads | as the parse pool | Bulk | already never the limit (1.15M events/s per writer) |
| Index owner + Tantivy writer threads | CPU | during an import: cores - 1 - parse threads, at most 4; after it: Auto (68f33b988) | Bulk, never `BACKGROUND`: the import's final drain waits on it (rule 52: work that is waited on is not starvable) | no Parquet re-read for teed batches; no counts in postings (6ae55bbf7); measure whether indexing the template beside the message adds words |
| Catalogue workers | CPU | during an import: what is left of cores - 1, at least 1; after it: as 71c18c57c | Bulk | entity extraction once, on batches already in memory; no DuckDB fetch (26% of its time) and no per-source re-open of segment files |
| Catch-up readers (Parquet) | SSD I/O + decode | 4 (`SsdIo`), 1 to 2 on HDD | Bulk | only skipped segments |
| Search, rows by id | | the caller's | Live | a live filter over two fast fields |

The cost to beat, from the baseline: parsing about 300 CPU-s (40 s x 7.5 threads), indexing about 336 CPU-s (168 s x 2
threads), the catalogue about 170 CPU-s (one thread). Overlapping them cannot remove that work, only fill the 5 to 6
cores the parse pool leaves idle on a 14-core machine and drop the re-reads. **Estimate (not measured):** with all of it
spread over 13 cores, everything could be ready around 60 to 80 s; the import itself will slow down by whatever CPU the
consumers take from the parse threads, which is why consumers skip instead of slowing the parse. Tantivy at about 330k
documents per second per two threads cannot keep up with 1.4M events per second, so most batches of a large import are
expected to be skipped and caught up afterwards; prototype A alone (more index threads after the import, now possible at
`UTILITY`) may give most of the gain with none of the consistency work. The measurements decide.

### 6. Migration of existing stores

No backward compatibility before v1, and the index and catalogue are rebuildable caches:

- `sources.gen` is added on open (`ALTER TABLE ... ADD COLUMN IF NOT EXISTS gen UBIGINT NOT NULL DEFAULT 0`, beside
  `ADDED_DDL`); existing sources are generation 0. `FORMAT_VERSION` does not change.
- The index `LAYOUT` goes from 4 to 5: an older index is recreated empty and every segment indexed again in the
  background (the existing epoch and layout check in `Fts::open_or_create`), with progress and Cancel as any reindex.
- The catalogue's `VERSION` goes to 2 only if the chunk-order merge changes its rows on `/samples`; then every source is
  built again in the background (the existing version check).
- Shared projects (ADR 0014 §6) carry the same caches: opening one rebuilds them in the background. Nothing is asked of
  the user; the release notes say the first open of an older store indexes again.

### 7. Prototypes and what they measure

Same machine, corpus and harness as the baseline (`tests/import_ready.rs` on `wip/debrief-duckdb2-prototypes`,
`large10`, release, quiet machine: load under 10 at start, three interleaved runs, warm and cold), recorded in
`performance.md` under rule 48.

- **A, parallel readers after the import.** No tee. After the import, the index owner reads segments with 4 readers and
  indexes with Auto writer threads at `UTILITY`; catalogue workers as 71c18c57c.
- **B, tee during the import.** Sections 3 and 4 with catch-up by today's single reader; the write lock still held by
  the import (the tee needs no other lock).
- **C, both.** B, with A's catch-up.

| Metric | How |
|---|---|
| Search ready | seconds from the start until every visible event is in a committed index generation |
| Catalogue ready | seconds until every ready source is catalogued |
| Import throughput | events per second until the last segment is listed; and seconds until positions exist |
| Memory | peak physical footprint during the import and after, and the tee's own share (queue bytes, writer heap, tallies) |
| Query latency | word-search p50 and p95 of 20 queries, during the import (from 10 s on) and after |
| Skips | share of batches the tee skipped, per queue |
| Cost | total CPU-seconds of the worker; index size and Tantivy segment count after merges |
| Correctness | on `/samples` and on `large03`: hit ids and order equal to A's (and to `ord` order), catalogue rows equal, for 1, 4 and 8 parse threads; a cancelled import and a killed worker reopen with no stale hit |

Adoption rule (the user's decision 2 below): B or C replaces A only if it brings search ready at least 25% earlier than
A, costs at most 15% of import throughput, keeps the tee within its 240 MB share and the word-search p95 during an import
under 0.5 s.

## Consequences

- The index and the catalogue no longer wait for the import to end; the user sees search working within seconds of the
  first sources becoming ready, and the gap to "everything searchable" is the catch-up of skipped segments.
- The append window (`gate.rs`) is dropped: nothing waits on background work inside the import, and the store's catalog
  has one writer during an import.
- Plan package P4 ("entity tally in the parse threads") changes: the tally runs on catalogue workers fed by the tee,
  because the parse threads are already CPU-bound and would slow the import by the extraction's cost. Package 5.2
  ("defer the index and catalogue for huge imports") becomes the low-memory, fan-out-off path. Both accepted
  2026-10-31; `docs/plans/debrief-1-2.md` (P4, 5.2 and the IX rows, IX-ADR being this ADR) says so.
- New: `sources.gen`, index fields `gen`, `line` and a FAST `source`, the live filter in every search, the drain and
  sweep rules, chunk-ordered tally merges. Each gets a test that triggers its failure (cancel mid-tee, kill before an
  index commit, a full queue, a reload while searching).
- Notes on events still lose their event when the source is read again; unchanged by this ADR.

## Work packages

| WP | What | Files | Role | Size (days) |
|---|---|---|---|---|
| WP-1 | Prototype A and its measurements; record in `performance.md` | `fts.rs` (readers), `catalogue.rs`, `tests/import_ready.rs` | researcher | 1.5 |
| WP-2 | Failing tests first: hit order equals `ord` order with equal times; a not-ready, aborted or older-generation source is never a hit; a kill between index commit and listing leaves no stale hit; a full queue never blocks `write` | `crates/debrief-store-duck/tests/`, `fts.rs` tests | implementer | 1 |
| WP-3 | Index layout 5 (`gen`, `line`, FAST `source`), the live filter and the total order collector; `sources.gen`; open-time sweep | `fts.rs`, `schema.rs`, `lib.rs` (open) | designer | 2 |
| WP-4 | The tee: bounded queues, skip and partial marks, the index owner consuming the queue, commit cadence, the sink marking `indexed`, drain and delete before compaction, cancel | `writer.rs`, `fts.rs`, `lib.rs` (`import_keeping`, `maintain`) | designer | 3 |
| WP-5 | Catalogue from the tee: per (source, chunk) tallies merged in chunk order, field counts with caps, rows written with the listing; differential test | `debrief-core/src/catalogue/`, `catalogue.rs`, `writer.rs` | designer | 2.5 |
| WP-6 | Memory shares and thread caps as Performance settings with Auto; tee off in low memory | `lib.rs` (`StoreOptions`), app settings schema | implementer | 1 |
| WP-7 | Prototypes B and C, the adoption decision, ADR to Accepted or amended | harness, `performance.md`, this ADR | researcher | 1.5 |

WP-1 runs first and alone; if A meets the target, WP-3 to WP-6 are reconsidered before they start.

## Decisions (user, 2026-10-31)

1. **Search lag during an import.** Search may show an event up to about 10 s after the event list does; the index
   commits about every 10 s during an import (section 3, commit point 2). No commit per listing.
2. **Adoption thresholds** (section 7), as written: B or C replaces A only if search is ready at least 25% sooner than
   with A, import throughput drops by at most 15%, the tee stays within its 240 MB share, and the word-search p95 during
   the import stays under 0.5 s.
3. **Notes after a reload are out of scope.** Re-anchoring a note's event after a reload by (source, line, excerpt) is
   a later, separate Debrief package, not part of this ADR or of IX; it is the only reason to want ids that repeat across
   imports, and nothing here depends on it. Pointer: this section, and the IX-ADR row of `docs/plans/debrief-1-2.md`.
4. **Plan changes** (accepted by the coordinator with the above): P4's entity tally runs on the catalogue workers fed by
   the tee, not in the parse threads; 5.2's deferral becomes the low-memory, fan-out-off path.
