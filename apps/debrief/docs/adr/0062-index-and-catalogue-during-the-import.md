# 62. Stable event ids: the search index and the catalogue are built during the import

- Status: Accepted (2026-10-30), after two independent reviews. Revision 2 answered the first review's P1 and P2
  findings; revision 3 the second review's P2 findings and the coordinator's decision to drop source generations.
- Date: 2026-10-30
- Related: ADR 0010 (persistent store), ADR 0014 (database-first engine; §2 step 4 already planned "index: Tantivy on
  its own thread, from the same batches"), ADR 0037 (live tail), ADR 0042 (port table), ADR 0051 (stable source ids),
  ADR 0052 (stage pipelines); Paddock rules 11, 13, 14, 26, 48, 51 and 52 (`docs/rules.md`; 51 and 52 are on `main`
  from ddf0becda and reach this branch with the next merge of `main` into `debrief/duckdb-2`); ER-18, ER-20; plan
  `docs/plans/debrief-1-2.md` packages P4, 5.2, SR1 to SR3 and IX

## Context

The user wants the import to do everything at once: each parsed batch goes, at the same time, to the Parquet writer,
the full-text indexer and the catalogue (names) aggregation. Today, on `debrief/duckdb-2` at 3659f43e8:

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
import wait for background work, which this ADR forbids. Plan package SR2 (cancel the steps when the window closes) is
the alternative fix of that prototype; this ADR replaces it if adopted (section 7).

### What the code already guarantees about ids (verified by reading)

- `EventId` is a `u64` (`debrief-batch/src/ids.rs`). The sink gives every segment a block of `SEGMENT_ROWS` (131,072)
  ids when it opens (`writer.rs`, `open_segment`); a batch gets consecutive ids from its segment's block in the parse
  thread, *before* it is written (`batch.assign_ids(EventId(w.next_id()))`). So a batch has its final ids at the moment
  it could be handed to another consumer.
- Ids are never reused once a segment holding them is listed: `meta.next_id` is persisted with every listing.
- Ids never change: compaction (`compact.rs`, `rewrite` uses `write_existing`) and the clock-skew rewrite (`skew.rs`,
  "the rewritten segments keep their id blocks") rewrite rows with their ids; a removed source's ids go with it.
- A segment's ids lie in `[first_id, end_id)` (the `segments` table), which is how `rows(ids)` and `texts(ids)` read only
  the segments that may hold the ids (`pos.rs`, `files_for`). A compacted segment's range is the hull of the blocks it
  merged, so it is not one block.
- The index stores the id, the source, a segment key and the time (`fts.rs`, `schema`: `id` FAST, `source` INDEXED,
  `seg` INDEXED = FNV of the segment's *file name*, `ts` FAST) and sorts hits by `ts` (`order_by_fast_field("ts")`). It
  has not used positions since index `LAYOUT` 2.
- Ids are not reproducible between two imports of the same files: which block a batch lands in depends on which parse
  thread wrote it. Within one batch the ids follow the file order. `pos` orders by `(coalesce(ts, MAX), source, line,
  id)`, so the id only breaks ties between events of the same source line, which come from one batch: page and
  position order are deterministic although the ids are not.
- Notes anchor an event by its id (`debrief-notes/src/anchor.rs`, `Anchor::Event`) with time, source and an excerpt
  shown when the id no longer resolves; a reload gives every event a new id. Re-anchoring such notes is out of scope
  (decision 3).

### Defects in today's code that this design would expose (verified by reading; the review found 1, 3 and 4)

1. **Lock order in the sink.** `open_segment` takes `ids` and, while holding it, `catalog` (to persist `next_id` when an
   observer is set). `list` runs with `catalog` held (by `register` or `finish`) and takes `ids`. The observer is unused
   today, so the inversion is latent; turning the tee on makes it a deadlock between a parse thread opening a segment
   and the thread listing.
2. **The index key is a file name.** `seg` is the hash of the segment file's name; compaction and the skew rewrite
   replace files, so documents written for one file can no longer be deleted by the key of the file that now holds
   their rows. Today this is harmless only because both keep the `indexed` flag and never re-index those rows.
3. **Clock skew leaves the index's times stale.** `skew.rs` says "the full-text index does not hold times, so it stays";
   it holds `ts` (FAST) and sorts by it, so after an offset change hits come back in the old time order.
4. **A reload empties the index.** `reload` sets the old data aside (`set_aside_events`: every `ASIDE_TABLES` table is
   copied to `aside_*` and emptied, the index cleared), imports, then either drops the copy (`discard_aside`) or puts it
   back (`restore_aside`: tables restored, every segment marked unindexed so it is indexed again). During a reload the
   store and search are empty; this ADR keeps that behaviour (section 1, "Re-imports").

So the stable id the user asked for exists. What is missing is everything around it: keys that survive rewrites, a way
to hide documents of rows that are not visible, consistency after a crash or a cancel, and an import that never waits.

## Decision

### 1. The id scheme: keep the block-allocated `EventId` as the one event identity

The event id stays what it is: a `u64` from per-segment blocks, assigned in the parse thread before the batch is
written, never reused, never changed. It is the primary key of an event in every layer (Parquet rows, `ord`, the index,
the catalogue's first and last ids, notes). No mapping table from id to row is added.

| Candidate | Unique for every importer | Known at parse time without a barrier | Fits `u64` | Keeps ids in reserved, disjoint blocks | Verdict |
|---|---|---|---|---|---|
| **Block id (today)** | yes | yes (the sink's block) | yes | yes | **kept** |
| Source id + byte offset | no: binary and legacy importers write `byte_offset` 0 (`ingest/legacy.rs`, `row.rs`); one line can give several events | yes | no: 32-bit source + 32-bit offset caps files at 4 GiB (decompressed archives exceed it) | no: a segment holds batches of many sources | rejected |
| Source id + line or record number | no: several events per line (JSON arrays, records split by importers); record ordinals need every earlier chunk's count | lines yes (the pre-scan), record ordinals no | only with a narrow source field | no | rejected |
| Import batch sequence + index in batch | yes | needs one global counter per batch (a contention point the block avoids) | yes | no: batches of one sequence spread over segments | rejected: the block id is the same idea with fewer holes and no shared counter per batch |
| 64-bit packed (source:24, ordinal:40) | yes if the ordinal is per source | no: a per-source ordinal over parallel chunks needs the counts of every earlier chunk (a barrier) | yes | no | rejected |

**Blocks are the stable keys of the index.** A new side table `segment_blocks(file, first_id, end_id)` lists every id
block a segment file holds. The sink writes one row per block when it lists a segment; compaction and the skew rewrite
move the rows of the files they replace to the new file in the same catalog transaction. Blocks are reserved
exclusively, so they never overlap, whatever file holds them. The index deletes "the documents of a block" with a range
query on `id` (`RangeQuery` on the FAST `id` field, Tantivy 0.27, `query/range_query/range_query_fastfield.rs`), which
does not depend on file names. The `seg` field goes.

**What the index stores per event (index `LAYOUT` 5):** `id` (FAST; range-queried), `ts` (FAST, untimed as
`i64::MAX`, as today), `source` (INDEXED and FAST) and `line` (FAST).

**From an id to a row or a position:** unchanged. Rows: `rows(ids)` and `texts(ids)` find the segments by their id
ranges and filter `READY`. Positions: `position_of(id)` reads `ord` and returns `None` until the event is positioned (the
port already says so).

**No source generations (YAGNI, coordinator decision 2026-10-30).** Revision 2 stamped documents with a per-source
generation. No current flow needs one: a reload clears the index before it reads again (below), a followed file read
again in full becomes a new source, and a deleted source simply stops being ready. A future build-new-then-swap reload
ADR would add `source_gen(source, gen)` and a `gen` field to the live filter; nothing here prevents that.

**Re-imports (reload)**, as the code does it (defect 4): the store and the index are empty while a reload runs and
fill as the new import lists segments, exactly as for a first import. With the tee the index receives the new import's
documents during the reload, so:

- `discard_aside` (success) changes nothing in the index: it holds only the new import's documents;
- `restore_aside` (failure or cancel) clears the index before it marks the restored segments unindexed (today it relies
  on the clear done by `set_aside_events`, which the tee would have refilled);
- the new side tables `segment_blocks` and `source_chunks` join `ASIDE_TABLES`, so they are set aside and restored
  with the rest. `catalogue`, `catalogue_trigrams` and `catalogue_sources` stay out, as today (rows are joined with ready
  sources).

A per-source build-new-then-swap reload (old events answer until the new ones are ready) is not in scope; it would get
its own ADR (with source generations), work package and tests.

**The tee guard.** `import_keeping` creates the tee as a guard value (`TeeGuard`). On every way out of the function,
success, an early `?` return, a panic unwound by `guard::catch`, or cancel, its `Drop` stops the queues, discards what
the index writer holds uncommitted (`rollback`), drops the tallies and releases the writer to the index owner. Because
the guard lives inside `import_keeping`, it is gone before `reload` calls `restore_aside`, so the restore's index clear
never meets a writer still filling. A test makes the sink fail after the tee started (a crash point returning an error)
and checks that `restore_aside` leaves no stale hit and that the next import can open the writer.

**Duplicates.** One file at two paths is two sources (ADR 0051 never guesses); a path already known in the import is
skipped (`known` in `import_keeping`). Within the index, a block's documents are only ever added after its range was
deleted in the same uncommitted writer state, or once by the tee (section 3), so no document is indexed twice.

**Append to an existing store (live tail, ADR 0037).** Appended events get new ids in a new segment of the same source
; the tee indexes them like an import's batches; `pos::append` extends positions
without a sort when they sort after the last positioned event, as today. A followed file that must be read again in full
becomes a new source (`tail.rs`) with new ids.

**Deleting a source.** As today: the catalog deletes the source (it stops being live at once, so its documents are
filtered out at once), then the index deletes `source = s` and the catalogue its rows. A crash between the two leaves
documents no live source owns; the open-time sweep (section 3) removes them.

### 2. Time order without a global sort before search

The index answers in time order by itself. `pos` stays a store-side concern for the unfiltered event list and jumps.

- Search sorts by `(ts, source, line, id)`, the order of `ord`. Today only `ts` orders the hits, so equal times come back
  in an order the code does not define. Tantivy 0.27 takes tuple sort keys in `TopDocs::order_by`
  (`collector/sort_key/sort_key_computer.rs`; confirmed by the first review); its cost against the single `ts` key is
  measured in prototype B.
- **The live filter** is one `DocPredicateQuery` (Tantivy 0.27, `query/doc_predicate_query/`) added as a `Must` clause of
  every search, so `Count`, `TopDocs` and the `Ids` collector behind the search box's filter all inherit it. A document
  is live when (a) its `source` is ready and (b) its `id` lies in a block of a listed segment. Both tables are read once per search under a snapshot (sources: thousands at most; blocks: a sorted vector,
  binary search). (b) is what hides documents the tee committed before the sink listed their segment.
- `SearchReply` gains `complete: bool` (`debrief-core/src/ports/dto.rs`; serde default `true` for older peers): false
  while some visible segment is not yet indexed, so the UI shows "Still indexing" next to counts that will grow
  (`exact` keeps its meaning: phrase candidates not all checked). A DTO field, not a new operation, so ADR 0042's
  append-only operation list is untouched.
- Mid-import, a hit whose event has no position yet still opens: its row comes from `rows(ids)`; the event list scrolls
  to its time (`position_of` is `None`, the UI anchors by time) until positions exist.
- The clock-skew rewrite marks the segments it rewrites unindexed (defect 3; the skew doc comment is corrected with it),
  so their documents are replaced with the new times. A test sets an offset and checks that hit order equals `ord` order.
- `pos::rebuild` stays at the end of the import; its cost is plan package SR4.

### 3. The pipeline

```
discover ─▶ dispatcher ─▶ parse pool (threads) ──▶ SegmentSink.write ── Parquet segment (always, in the parse thread)
                                                         │ observe(&batch, chunk) after the write, no lock held
                                                         ├─ try_send ─▶ index queue (bounded) ─▶ the index owner thread
                                                         └─ try_send ─▶ names queue (bounded) ─▶ catalogue workers
```

**Lock order in the sink** (fixes defect 1). The sink's locks are taken in one order: a parse thread's `slot`, then
`catalog`, then `ids` (`write` holds its slot while `register` tries `catalog` and `open_segment` takes `ids`; `finish`
empties the slots before it lists). No lock is taken against that order, and none is held across an `observe` call.
`open_segment` takes only `ids`. Persisting ids moves to the listing:

- the listing (which holds `catalog`, then takes `ids`) persists `meta.next_id` as the current next id plus a
  reserve of `RESERVE_BLOCKS` blocks (64 x 131,072 ids);
- `open_segment` hands out blocks below that persisted mark without touching `catalog`; a block beyond it (an import
  faster than the listing) drops `ids`, takes `catalog` then `ids`, and persists a new mark first;
- the order is written at the top of `writer.rs` and checked by a stress test: 16 threads, a segment size of 8 rows and
  `LIST_EVERY` of 0, many tiny sources, under a deadline that fails the test instead of hanging. `SEGMENT_ROWS` becomes
  a test seam for this: the sink takes its segment size from its options (default `SEGMENT_ROWS`), as it already takes
  `list_every`.

**One owner per store artefact.** The index has one writer (Tantivy allows one); its owner is the `FtsService` thread,
which during an import consumes the index queue and otherwise catches up from Parquet. The catalogue workers (prototype
71c18c57c, plan SR3) consume the names queue. During an import the sink is the only writer of the catalog's import
tables (`segments`, `segment_blocks`, `sources`, `source_chunks`, `meta`): it records index progress in its own listing
transactions. The catalogue workers write only the catalogue tables, each source in its own transaction gated on the
source being ready, as `catalogue.rs` `commit` does today; the two never write the same rows. Nothing in this design takes the store's
write lock beside the import, and the append window of `gate.rs` is not needed.

**The import never waits for a consumer.** `observe` (called after the segment write with the slot released)
`try_send`s an `Arc` of the batch with its chunk index (section "Catalogue determinism"). When a queue is full the batch
is skipped:

- index: its block is marked *partial*; later batches of that block skip too;
- catalogue: its source is marked *partial*; its tallies are dropped and the source is built after the import.

**Catch-up.** A partial or skipped segment stays unindexed (`segments.indexed = FALSE`); the index owner indexes it from
Parquet after the import, deleting each of its blocks' ranges before adding (plan SR1 makes this reader parallel). A
partial source goes to the catalogue's `pending()` list as today.

**Compaction never merges unindexed segments.** It groups indexed segments only; an unindexed one is compacted by a
later `maintain`, after it is indexed. Together with block keys this makes "delete then add" exact whatever compaction
did meanwhile.

**Commit points.**

1. Parquet: a segment is listed (with its sources made ready and its blocks in `segment_blocks`) every `LIST_EVERY`
   (1 s), as today.
2. Index: the owner commits about every 10 s (decision 1) or 512 MB of input text, whichever is first, and at the end
   of the import. After each commit it hands the sink the blocks now complete in the index; the sink marks a segment
   `indexed` in its next listing transaction once all its blocks are complete. A document committed before its segment
   is listed is not live (filter (b)).
3. Catalogue: a source's tallies are merged when the source is finished; a catalogue worker writes its rows in its
   own transaction once the source is ready (a worker whose source is not ready yet waits for the next listing's wake),
   so a lookup sees its names at most one listing after its events. Nothing of the catalogue runs in the listing, so the
   listing's time stays what it is today; prototype B records it as a metric.
4. End of import: drain the queues (bounded, so this wait is short; its time is a measured metric), final index commit,
   last listing, and the tee guard is dropped; then **release the tee's memory** (the owner waits for Tantivy's merges and drops the writer, the
   catalogue drops its tallies, `debrief_alloc::release()`) **before** `maintain` (compaction, `pos::rebuild`,
   checkpoint), so the ordering's DuckDB memory never stacks on the tee's; catch-up of skipped segments starts after
   `pos::rebuild`.

**Catalogue determinism.** A tally is capped, so what enters it depends on the order of events. The rule:

- The order of a source's events is (chunk index, position in the chunk). The chunk plan depends only on the file and
  the chunk size (`ingest/plan.rs`), not on threads; a small or compressed file is one chunk. The sink's `observe`
  passes the chunk index (a new field the parse side already knows), and the sink records each chunked source's chunk
  starts in a side table `source_chunks(source, chunk, first_byte, first_line)` (from `ChunkStart`) with the listing.
- Within a chunk, ids increase in event order: one chunk is parsed on one thread, which writes to its own slot, and a
  slot's next block always lies above its last. A test checks that ids increase with `line` inside every chunk.
- Caps: each chunk's tally keeps at most `MAX_NAMES` names in first-seen order; tallies are merged in chunk order, a
  name enters the source's tally only while it holds fewer than `MAX_NAMES`. `MAX_EVENTS_SCANNED` is applied at chunk
  granularity in both paths: chunks are taken in order while the events taken so far are below the cap, a chunk that
  starts below it is taken whole, and later chunks are not scanned. When `dropped` is 0 every count is exact; otherwise counts are lower bounds, as the `dropped` column
  already tells the UI.
- The post-import build (skipped sources, older stores) computes the same function without relying on byte offsets: a
  source with `source_chunks` rows is split by `line` at each chunk's `first_line` and read `ORDER BY chunk, id`; a source without rows (one chunk) is read `ORDER BY id`. The oracle in the differential test does the same; tests run
  with 1, 4 and 8 parse threads and with a chunk size small enough to split `/samples` files.
- Field values (today a SQL `GROUP BY` over `extra`) become per-source counts with the same caps, plus a new bound of
  1,000 keys per source.

**Cancel.** The import's cancel stops the tee with the parse threads. Sources finished before the cancel stay (today's
rule: what was read stays usable) and keep their documents and tallies; aborted sources are deleted from `sources`, so
their documents are not live at once and are deleted in the drain commit; their tallies are dropped.

**Crash and the open-time sweep.** Before anything else writes, opening a store deletes from the index, in one commit,
every document that is not live or belongs to an unindexed segment: one `delete_query` with a `DocPredicateQuery` that
is true when the `id` is in no block of an indexed listed segment or the `source` is not in `sources`. The sweep logs
the segment and block counts it checked and its time, so its cost on large stores is on record. Those
segments are then indexed again. Because the sweep runs before any import, an id range that a crashed import used and
did not persist can be reused afterwards without meeting a stale document.

**What search shows mid-import.** Live documents only: events of ready sources, in listed segments, in a committed index
commit, at most about 10 s behind the event list (decision 1), plus whatever the tee skipped, which appears as the
catch-up proceeds. `complete` stays false until then, and `index_status` reports the segments still to index (rule 11).

**Testability hooks** (test-only features of `debrief-store-duck`): named crash points (after an index commit and before
the listing, after the listing and before the index commit, inside the drain); queue capacity 0, which must give exactly
prototype A's result; the sink's segment size (above); a property test that imports random small sources with random times, offsets and cancel points
and checks that search hits, in order, equal the `ord` order of the matching events.

### 4. Bounded resources and memory

The store worker's import budget is 768 MB (ADR 0014); measured peaks are already 905 to 979 MB physical footprint on the
2.42 GB corpus (`performance.md`), and plan package 4.3 owns cutting them. The tee gets a fixed share of that budget,
taken from the import memory setting, never added to it:

| Part | Bound | Default share |
|---|---|---|
| Index queue | batches and bytes of the Arrow buffers it keeps alive | 16 batches, 32 MB |
| Documents being built | the owner's document pool (`debrief-fts-docs`, up to 4 threads) holds at most 2 batches of documents per thread | 16 MB |
| Index writer heap | threads x heap per thread (Tantivy needs at least 15 MB per thread) | 4 x 24 MB = 96 MB |
| Tantivy merges during the import | merge threads read and write segments; the merge policy is limited to small segments while importing (large merges wait for the end) | 32 MB |
| Names queue | batches and bytes | 8 batches, 16 MB |
| Catalogue tallies in flight | bytes over all (source, chunk) tallies; at the cap the largest source's tallies are dropped and the source built later | 48 MB |
| Total | | 240 MB |

The share is released before `maintain` (section 3, commit point 4). The tee is off, and the import behaves as today,
when the import memory leaves less than this beside the parse pool (low memory mode, or a smaller setting) or when no
core is left for it (section 5). The prototypes measure physical footprint (`phys_footprint` on macOS, PSS on Linux),
not RSS (rule 26).

### 5. Parallelism and cost (ER-20, rule 51)

| Stage | Kind | Threads (default, a Performance setting with Auto) | Role (rule 52) | Cheaper? |
|---|---|---|---|---|
| Parse pool | CPU | `min(cores - 1, 8)` (unchanged) | Bulk: `UTILITY` / nice 10 / `BELOW_NORMAL` | regex tail split (433565981), about 15 to 20% less parse CPU expected, not measured end to end |
| Parquet write | CPU, in the parse threads | as the parse pool | Bulk | already never the limit (1.15M events/s per writer) |
| Index owner + Tantivy writer threads | CPU | during an import: `spare = cores - 1 - parse threads`, the writer gets `min(spare, 4)`; after it: Auto (68f33b988) | Bulk, never `BACKGROUND`: the import's final drain waits on it | no Parquet re-read for teed batches; no counts in postings (6ae55bbf7); measure whether indexing the template beside the message adds words |
| Catalogue workers | CPU | during an import: what `spare` leaves, at least 1; after it: as SR3 | Bulk | entity extraction once, on batches already in memory; no DuckDB fetch (26% of its time) and no per-source re-open of segment files |
| Catch-up readers (Parquet) | SSD I/O + decode | 4 (`SsdIo`), 1 to 2 on HDD (SR1) | Bulk | only skipped segments |
| Search, rows by id | | the caller's | Live | the live filter over fast fields |

**When `spare` is 0 or less** (for example 8 parse threads on a machine with 9 cores or fewer), the tee is off: the
consumers would only take CPU from the parse threads.

The cost to beat, from the baseline: parsing about 300 CPU-s (40 s x 7.5 threads), indexing about 336 CPU-s (168 s x 2
threads), the catalogue about 170 CPU-s (one thread). Overlapping them cannot remove that work, only fill the 5 cores
the parse pool leaves idle on a 14-core machine and drop the re-reads. **Estimate (not measured):** everything could be
ready around 60 to 80 s at best; the import slows by whatever CPU the consumers take from it. Tantivy at about 330k
documents per second per two threads cannot keep up with 1.4M events per second, so most batches of a large import are
expected to be skipped and caught up afterwards; prototype A alone (more index threads and parallel readers after the
import) may give most of the gain with none of the consistency work. The measurements decide.

### 6. Migration of existing stores

No backward compatibility before v1, and the index and catalogue are rebuildable caches. No existing table changes:

- `segment_blocks` and `source_chunks` are side tables in `ADDED_DDL` (`CREATE TABLE IF NOT EXISTS`, no
  `ALTER`), so the appenders and `INSERT ... SELECT *` statements over `sources` and `segments`, and the aside copies,
  keep their column lists. `FORMAT_VERSION` does not change.
- On open, a store without `segment_blocks` rows takes them straight from `segments` for every segment whose
  `[first_id, end_id)` overlaps no other segment's range (every never-compacted segment, and compacted ones whose hull
  is clear: ids in its gaps belonged to removed rows and are never reused). Only segments whose ranges overlap are
  scanned for runs of consecutive ids (`SELECT id - row_number() OVER (ORDER BY id) ...` per file), as a background step
  with progress and Cancel, before their segments are indexed. A source without `source_chunks` rows is one chunk.
- The index `LAYOUT` goes from 4 to 5: an older index is recreated empty and every segment indexed again in the
  background (the existing epoch and layout check in `Fts::open_or_create`), with progress and Cancel.
- The catalogue's `VERSION` goes to 2 (the order and cap rule above), so every source is built again in the background.
- Shared projects (ADR 0014 §6) carry the same caches: opening one rebuilds them. The release notes say the first open
  of an older store indexes again.

### 7. Prototypes and what they measure

Same machine, corpus and harness as the baseline (`tests/import_ready.rs` on `wip/debrief-duckdb2-prototypes`,
`large10`, release, quiet machine: load under 10 at start, three interleaved runs, warm and cold), recorded in
`performance.md` under rule 48.

- **A, parallel readers after the import.** No tee. After the import, the index owner reads segments with 4 readers and
  indexes with Auto writer threads at `UTILITY` (plan SR1); catalogue workers as SR3.
- **B, tee during the import.** Sections 1 to 4, with catch-up by today's single reader.
- **C, both.** B, with A's catch-up.

| Metric | How |
|---|---|
| Search ready | seconds from the start until every visible event is in a committed index commit (`complete` true) |
| Catalogue ready | seconds until every ready source is catalogued |
| Import throughput | events per second until the last segment is listed; and seconds until positions exist |
| Memory | peak physical footprint of the whole worker during the import, during `maintain` and during catch-up; and the tee's own share (queue bytes, documents, writer heap, merges, tallies) |
| Listing | time per listing transaction during the import, against today's (the catalogue no longer writes in it) |
| Drain | seconds from the last parsed batch to the final index commit (bounded: a failure above 5 s) |
| Query latency | word-search p50 and p95 of 20 queries, during the import (from 10 s on) and after |
| Skips | share of batches the tee skipped, per queue |
| Cost | total CPU-seconds of the worker; index size and Tantivy segment count after merges |
| Correctness | the property test and the crash points of section 3; on `/samples` and `large03`, hit ids and order equal to A's and to `ord`, catalogue rows equal, for 1, 4 and 8 parse threads |

**Adoption rule** (decision 2, with the review's memory criterion): B or C replaces A only if search is ready at least
25% sooner than with A, import throughput drops by at most 15%, the tee stays within its 240 MB share, the word-search
p95 during the import stays under 0.5 s, and the worker's total peak physical footprint is no higher than A's.

## Consequences

- The index and the catalogue no longer wait for the import to end; search works within seconds of the first sources
  becoming ready, and the gap to "everything searchable" is the catch-up of skipped segments.
- The append window (`gate.rs`) and plan package SR2 are dropped if B or C is adopted; SR1 becomes A and the catch-up;
  SR3 becomes the catalogue workers of the tee and of the post-import build.
- Plan package P4 changes: the tally runs on catalogue workers fed by the tee, not in the parse threads. Package 5.2
  becomes the low-memory, fan-out-off path. Both accepted on 2026-10-30 (decision 4).
- Three latent defects are fixed on the way (lock order in the sink, file-name index keys, stale times after a clock
  offset), each with a failing test first.
- New: `segment_blocks`, `source_chunks`; index `LAYOUT` 5; the live filter; `SearchReply.complete`; the sweep on
  open; the tee guard; chunk-ordered tallies. Each gets a test that triggers its failure (cancel mid-tee, crash points, a full
  queue, a reload that fails while the tee runs).

## Work packages

| WP | What | Files | Role | Size (days) |
|---|---|---|---|---|
| WP-1 | Prototype A and its measurements (independent of the rest; may start now); record in `performance.md` | `fts.rs` (readers), `catalogue.rs`, `tests/import_ready.rs` | researcher | 1.5 |
| WP-2 | Failing tests first: the 16-thread sink stress test (lock order); hit order equals `ord` with equal times and after a clock offset; a document of an unready, aborted or unlisted source is never counted or returned; a failed reload with the tee leaves no stale hit; an early error return from `import_keeping` drops the tee guard before `restore_aside`; a full queue never blocks `write` | `crates/debrief-store-duck/tests/`, `writer.rs` and `fts.rs` tests | implementer | 1.5 |
| WP-3 | Sink lock order (slot, catalog, ids), the id reserve and the segment-size seam; `segment_blocks` kept by listing, compaction and skew; compaction skips unindexed segments; skew marks rewritten segments unindexed and its doc comment is corrected | `writer.rs`, `compact.rs`, `skew.rs`, `schema.rs` | designer | 2 |
| WP-4 | Index `LAYOUT` 5, block-range deletes, the live filter as a `DocPredicateQuery`, the total order, `SearchReply.complete`; the open-time sweep as one predicate delete; `restore_aside` clears the index and the side tables join `ASIDE_TABLES`; the `segment_blocks` backfill (direct from `segments`, scan only overlapping ranges in the background) | `fts.rs`, `lib.rs` (open, aside), `debrief-core/src/ports/dto.rs` | designer | 2 |
| WP-5 | The tee: bounded queues, skips, the index owner consuming the queue, commit cadence, the sink marking `indexed`, drain, the tee guard on every exit path, memory release before `maintain`, cancel, crash points | `writer.rs`, `fts.rs`, `lib.rs` (`import_keeping`, `maintain`) | designer | 3.25 |
| WP-6 | Catalogue from the tee: chunk index through `observe`, `source_chunks` with `first_line`, per-chunk tallies merged in chunk order with the cap rule (`MAX_EVENTS_SCANNED` by whole chunks), rows written by the workers in their own transactions gated on ready, field counts with caps, the ordered post-import build, the differential test | `debrief-core/src/catalogue/`, `debrief-core/src/ingest/` (chunk index), `catalogue.rs`, `writer.rs` | designer | 3 |
| WP-7 | Memory shares and thread rules as Performance settings with Auto; tee off in low memory and when no core is spare | `lib.rs` (`StoreOptions`), app settings schema | implementer | 1 |
| WP-8 | Prototypes B and C, the adoption decision, this ADR amended with the numbers | harness, `performance.md`, this ADR | researcher | 1.5 |

WP-1 runs first; if A meets the target, WP-4 to WP-7 are reconsidered before they start. WP-3's fixes are worth having
either way.

In `docs/plans/debrief-1-2.md` (each day counted once): WP-1 is SR1 (which absorbs IX0 and IX-A); WP-2 and WP-3 are
IX-B1; WP-4 and WP-5 are IX-B2; WP-6 is P4 (the tee's tally) with SR3 (the workers); joining C is IX-C; WP-7 is IX-S;
WP-8 is IX-M. SR2 is superseded if B or C is adopted.

## Decisions (user, 2026-10-30)

1. **Search lag during an import.** Search may show an event up to about 10 s after the event list does; the index
   commits about every 10 s during an import (section 3, commit point 2). No commit per listing.
2. **Adoption thresholds** (section 7), as written: B or C replaces A only if search is ready at least 25% sooner than
   with A, import throughput drops by at most 15%, the tee stays within its 240 MB share, and the word-search p95 during
   the import stays under 0.5 s. (Revision 2 adds the review's criterion: total peak footprint no higher than A's.)
3. **Notes after a reload are out of scope.** Re-anchoring a note's event after a reload by (source, line, excerpt) is
   a later, separate Debrief package, not part of this ADR or of IX; nothing here depends on it. Pointer: this section,
   and the IX-ADR row of `docs/plans/debrief-1-2.md`.
4. **Plan changes** (accepted with the above): P4's entity tally runs on the catalogue workers fed by the tee, not in
   the parse threads; 5.2's deferral becomes the low-memory, fan-out-off path.
