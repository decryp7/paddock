# Search performance: Tantivy and Model2Vec (2026-10-28)

Question (user): "improve tantivy and model2vec for best performance". Method: measure first, profile, then change only
what the numbers support. Status: **measured** on 2026-10-29 between 04:53 and 05:17 in the unattended window, every run started at 1-minute load
below 12 (the load before and after each run is in the results blocks). The load still moved during runs (6 to 17, other
agents' checks), so single numbers carry about 10 to 15% noise; read ranges and A/B pairs. The commands are in
[Reproduction](#reproduction). The recorded blocks are also in `apps/debrief/docs/performance.md`
([2026-10-29, search performance](../performance.md#2026-10-29-search-performance-tantivy-steps-writer-grid-end-to-end-and-model2vec-branch-debriefsearch-perf)).
Earlier numbers marked *(load n)* were taken under heavy load and are indications only.

Machine: Mac mini M4 Pro, 14 cores, 64 GB. Branch `debrief/search-perf` from `origin/main` at `cd13d2355`. No user data:
the Tantivy benchmark generates its events (ER-14); the Model2Vec set is the 505 events and 36 labelled queries derived
from Debrief's generated samples (`docs/research/2026-10-embeddings-for-logs.md`, branch `docs/embeddings-for-logs`).
Memory is real memory (rule 26): macOS physical footprint.

## What exists

- **Tantivy 0.27.0** (MIT, released 2026-10-17, <https://crates.io/crates/tantivy/0.27.0>), features `mmap` and
  `lz4-compression`, in `crates/debrief-store-duck/src/fts.rs` (read):
  - Schema: `id` and `ts` fast fields, `source` and `seg` indexed (deletes by segment or source), one `text` field with
    the words of message, exception and template (`debrief_core::words`, rule 20). No stored fields, no positions
    (phrases are checked on the rows). The doc store is therefore empty: **lz4 vs zstd does not matter** here.
  - Writer: 64 MB heap, 2 threads (`WRITER_HEAP`, `WRITER_THREADS`; low-memory mode 16 MB, 1 thread); a commit per 4
    segments of 131,072 rows; the default `LogMergePolicy` (merges of 8 segments per level, up to 10M docs).
  - Reader: `ReloadPolicy::Manual`, reloaded after each commit; `searcher()` per search is an `Arc` clone.
  - Search: every word must match, the last word typed is a prefix (so nearly every keystroke is a prefix search);
    `TopDocs::order_by_fast_field("ts")` plus `Count`; `all_ids` hands up to 2M ids to DuckDB, which applies the time,
    level and other filters. Tantivy itself never filters by time or level, so extra fast fields would not be read.
- **Model2Vec: not built in Debrief.** No crate, dependency or branch uses it (all debrief, names, embed, search and
  pacenotes branches checked). Decided 2026-10-25: semantic search over message templates (Model2Vec built in, Qwen3
  through Pitwall), fused with BM25. `debrief/nl-search` (deterministic natural-language queries in `debrief-core/src/nl/`)
  does not touch embeddings or Tantivy; it is the natural router in front of lexical vs semantic search.

## Tantivy

### Benchmark

`crates/debrief-store-duck/src/fts_bench.rs` (ignored test): seeded events from 600 templates with Zipf-like frequencies,
GUIDs, hex ids, paths, durations, IPs, users, URLs, 3% .NET stack traces; 160 bytes and 42 words per event on average. It
builds through the real `Fts` (schema, tokenizer, writer budget, commit rhythm), then times 12 typical searches (terms,
prefixes of 1 to 4 letters, a hex prefix, a user id) for `search` (count + top 100 by time) and `all_ids`. It runs on
mimalloc, the store worker's allocator. The existing scale benchmark (`tests/bench.rs`) covers the whole pipeline
(import, Parquet read, document pool, writer).

### Baseline (indications)

| | 200k docs *(load 19)* | 1M docs *(load 31)* |
|---|---|---|
| Build | 65k docs/s, 11.1 MB/s of text | 50k docs/s, 8.5 MB/s |
| Peak footprint | 111 MB | 137 MB (merges 126 MB) |
| Commits / final merges | 0.9 s / 0 s | 0.5 s / 1.5 s, 12 → 5 segments |
| Index size | 12 MB, 60 bytes/doc | 56 MB, 56 bytes/doc |
| Tokenizer alone, 1 thread | 128k events/s, 20.5 MB/s | |

Searches on 200k docs, p50 over 30 runs: terms 0.01 to 0.35 ms; prefixes `conn` 0.69 ms, `pa` 0.73 ms, `t` 1.49 ms
(134k hits); the hex prefix `a3f` 2.22 ms (each GUID run is its own term, so a short hex prefix expands to thousands of
terms). `all_ids` 0 to 2.5 ms.

### Profile

macOS `sample`, 12 s during the 1M build: in each Tantivy indexing thread **71% of the samples (2,678 of 3,785) are in
our tokenizer**, most of it allocation: `words::parts` collects a `Vec<(usize, char)>` per run, every word is
`to_lowercase`d into a new `String`, the tokenizer builds a new `Vec<Token>` per text. Postings (`subscribe`) are most of
the rest. Only 2 indexing threads exist, and the thread feeding documents is mostly blocked sending to them. So the levers
are the tokenizer's allocations and the writer's thread count; merges, commits and the doc store are small.

### Changes

| Commit | Change | Held by | Measured |
|---|---|---|---|
| `e9a65e6b9` | `words_ref`: ASCII text lower-cased once, words handed out as slices, no `String` per word; other text takes the old path (`words_general`), so index and oracle still share one implementation | proptests (2,256 cases) and fixed cases: ASCII path = general path | Tokenizer 220k to 501k events/s; build 144k/168k to 203k docs/s (10M docs, 2 threads x 64 MB) |
| `e9a65e6b9` | Tokenizer reuses its tokens between texts (at most 4,096 kept) | unit test | included in the line above (the two were measured together) |
| `e9a65e6b9` | Prefix = term range `[p, p + U+10FFFF)` instead of a regex compiled per search | test: same hits as `p.*` on ASCII and accented words | No measurable change: prefix `pa` p50 16.6 ms (before) vs 16.1 ms (after) |
| `e9a65e6b9` | `search` opens the id column once per segment, not per hit; `all_ids` reads blocks of 64 hits (`collect_block`, `first_vals`) | search and differential integration tests | Term queries p50 15 to 17 ms to 0.7 to 2.5 ms; prefix `pa`/`conn` 30/29 ms to 16.6/14.9 ms; build 203k to 217k docs/s |
| `69b35c28a` | Words without counts or fieldnorms, `source`/`seg` without fieldnorms (nothing is scored); layout 5, existing indexes rebuild once | unit and integration tests | Build 218k to 308k docs/s; index 537 to 428 MB (53.7 to 42.8 bytes/doc); build peak footprint 313 to 203 MB; no query got slower (see the conclusions) |
| `dcb70b699` | The `debrief-fts` thread runs at background priority; Tantivy's indexing and merge threads start from it and inherit it (macOS QoS, Linux nice) | | Build speed unchanged by it: head 309k and 305k docs/s vs 308k for the build before it |

Not changed, with reason: the merge policy (final merges were 1.5 s per 1M docs and 5 segments remained; revisit only if the
10M run shows many segments slowing prefixes); mmap vs RAM directory (searches are already sub-millisecond on mmap and the
OS cache, and a RAM copy would add the index to the process's real memory); the collector (`TopDocs` by the `ts` fast
field is what the grid needs); the doc store codec (empty). Dropping the `lz4-compression` feature would only shrink the
binary; not worth a change by itself.

### Writer budget

Tokenizing runs inside Tantivy's indexing threads, so build speed should scale with writer threads until the feeder or
Parquet reading limits it. Tantivy needs at least 15 MB per thread; ADR 0014's budget is 64 MB. Grid on the head
binary, 10M docs, one build per cell (docs/s, peak footprint; load in brackets). A cell was skipped where heap per thread
would be under 15 MB.

| Threads | 64 MB | 128 MB | 256 MB |
|---|---|---|---|
| 2 | 312.7k, 193 MB (6.9) | 360.4k, 259 MB (6.8) | 361.5k, 254 MB (6.6) |
| 4 | 291.5k, 282 MB (5.9) | 357.4k, 435 MB (6.6 to 9.8) | 297.0k, 408 MB (9.5 to 14.9) |
| 8 | skipped | 284.2k, 415 MB (12.0 to 13.6) | **448.7k, 501 MB** (10.9 to 10.4) |
| 13 | skipped | skipped | 396.1k, 615 MB (9.9 to 13.0) |

Segments after the build: 12 to 19 for 2 threads, 10 to 24 for 4, 6 to 13 for 8, 16 for 13. The 4-thread/256 MB and
8-thread/128 MB cells ran while the load rose to 13 to 15, so they are likely low; the grid is one build per cell and is
indicative, not a ranking of neighbours. What is robust: (a) heap per thread matters more than thread count: every cell
with at least 32 MB per thread reached 313k to 449k docs/s, the cells at 15 to 20 MB per thread were slower or equal to 2
threads at 64 MB (284k to 396k); (b) more threads cost footprint (193 MB to 615 MB) for at most 1.4x speed.

**Proposal.** Writer heap of **32 MB per thread** (twice Tantivy's minimum), threads = `min(threads(WorkKind::Cpu), 8)`
(cores - 1, ER-20, capped at 8 because 13 threads were slower than 8 and cost 120 MB more). On this machine: 8 threads x
32 MB = 256 MB heap, 449k docs/s (+45% over today's 2 x 32 MB = 64 MB at 309k), peak footprint about 500 MB at 10M docs
(today 205 MB). Small machines (2 to 4 cores) get 64 to 128 MB. This is provisional: before adopting it, repeat the
t=8/256, t=4/128 and t=2/128 cells twice, interleaved, at load below 8. If the +300 MB peak is not acceptable for the
background indexer, the fallback is 2 threads x 64 MB (128 MB heap): 360k docs/s (+16%) at 259 MB.

### Found on the way

- `tests/bench.rs` reported `memory-stats`' `physical_mem`, which is the resident size on macOS, against rule 26. It now
  reports the physical footprint (macOS) or PSS (Linux) (`tests/common/rss.rs::real`). The memory budget tests
  (`tests/memory.rs`, `tests/analysis_memory.rs`) still use the resident size: a follow-up, since changing their measure
  changes what their budgets mean.
- A writer created outside the `debrief-fts` thread (`delete_source`, `clear` from a command) still starts its threads at
  normal priority; these writers index nothing, only delete.

## Model2Vec

### Crates (read 2026-10-28)

| Crate | Version, date | Licence | Notes |
|---|---|---|---|
| `model2vec-rs` (official, MinishLab) | 0.3.0, 2026-09-19 | MIT (`LICENSE`; crates.io shows "non-standard", cargo-deny needs a clarify entry) | <https://github.com/MinishLab/model2vec-rs> |
| `tokenizers` | 0.21 (used by model2vec-rs); 0.23.2, 2026-10-19 | Apache-2.0 | |
| `usearch` | 2.26.6, 2026-10-25 | Apache-2.0 | C++ HNSW |
| `hnsw_rs` | 0.3.5, 2026-10-09 | MIT/Apache-2.0 | pure Rust HNSW |
| `arroy` | 0.9.0, 2026-10-22 | MIT | LMDB-backed ANN; brings a database, not considered further |
| Models `minishlab/potion-code-16M-v2`, `potion-retrieval-32M`, `potion-base-8M` | Hugging Face | MIT (model cards) | 63,457 × 256 f16; 63,091 × 512 f32; 29,528 × 256 f32 |

Read in model2vec-rs 0.3.0's source (`src/model.rs`): it decodes the weights to f32 (an f16 model takes twice its file in
RAM), tokenizes with `encode_batch_fast`, which runs on rayon's global pool (all cores, unbounded, against ER-20 unless
wrapped), and calls `expect("tokenization failed")` (a panic on a tokenizer error, which is why the decision put it in a
guarded worker).

### Lean encoder

The spike (`2026-10-28-search-performance/m2v-spike`) has a lean encoder of about 150 lines: the same tokenizer, a lookup
table in f32, f16 or int8 (one scale per row), mean pooling and normalisation, no panics (a refused text gives a zero
vector), run in a bounded pool. It reproduces the earlier note exactly (potion-code-16M-v2 alone: plain-language recall@5
0.429, hit@5 0.600, MRR@10 0.522; identifier recall@5 0.800).

**Quality by table precision (measured, not load-dependent):**

| Model, table | Table | Plain hit@5 (m2v / BM25+m2v RRF) | Identifier hit@5 (m2v / RRF) | Smallest cosine to f32 |
|---|---|---|---|---|
| potion-code-16M-v2, f32 | 62 MB | 0.600 / 0.700 | 0.833 / 0.833 | 1 |
| same, f16 | 31 MB | 0.600 / 0.700 | 0.833 / 0.833 | 1.0000 |
| same, int8 + row scale | 16 MB | 0.600 / 0.700 | 0.833 / 0.833 | 0.9999 |
| potion-retrieval-32M, f32 | 123 MB | 0.600 / 0.767 | 0.667 / 0.667 | 1 |
| same, int8 | 31 MB | 0.600 / 0.767 | 0.667 / 0.667 | (MRR 0.420 vs 0.426) |

Every metric is the same for f16 and int8 (one MRR differs in the third decimal), so **int8 tables keep the quality at a
quarter of the memory**. Events in Japanese tokenise to unknown tokens only and get a zero vector in every precision (the
models are English; that is what Qwen3 through Pitwall is for). RRF of two top-100 lists: about 6 µs per query.

### Encode and search speed (measured 2026-10-29, load 8 to 12)

Encode, 100,000 templates of 61 bytes (texts/s; "tokenizing" is the share of time in the tokenizer):

| Model | Threads | model2vec-rs | lean f32 | lean f16 | lean i8 |
|---|---|---|---|---|---|
| potion-code-16M-v2 | 1 | 58.5k | 76.5k (94% tokenizing) | 71.3k | 77.9k (96%) |
| potion-code-16M-v2 | 13 | 133k | 348k | 350k | 364k (99%) |
| potion-retrieval-32M | 1 | 54.3k | 68.7k (92%) | 61.1k | 70.2k (92%) |
| potion-retrieval-32M | 13 | 115k | 303k | 303k | 332k (93%) |

Memory (peak above start): model2vec-rs +231 MB (16M) and +482 MB (32M); lean i8 +16 MB and +31 MB; lean f32 +160 MB
(16M, 1 thread). Load time 30 to 50 ms for lean, 77 to 167 ms for model2vec-rs. Tokenizing is 90 to 99% of the lean
encoder, so the table precision hardly changes the speed and int8 is free. The lean encoder is 1.3x faster than
model2vec-rs on one thread and 2.6x on 13. Quality is unchanged from the earlier run (re-run `m2v-spike quality`: same
recall, hit and MRR in f32, f16 and i8; min cosine to f32 0.9999 for i8).

Exact top-k search, 256 dimensions, p50 ms (recall@10):

| Vectors | f32 8-lane | f32 parallel (13) | int8 | int8 + f32 rescore | usearch HNSW f32 | hnsw_rs |
|---|---|---|---|---|---|---|
| 1,000 | 0.038 | 0.053 | 0.023 (0.964) | 0.058 (1.000) | 0.076 (0.956) | 0.146 (1.000) |
| 10,000 | 0.212 | 0.231 | 0.107 (0.862) | 0.203 (1.000) | 0.071 (0.996) | 0.164 (1.000) |
| 100,000 | 1.842 | 0.570 | 0.823 (0.818) | 1.052 (1.000) | 0.290 (0.996), build 2.3 s | 0.380 (0.908), build 4.2 s |
| 1,000,000 | 17.3 | 5.5 | 8.8 (0.818) | 8.2 (1.000) | 0.858 (0.772), build 96 s, +1.2 GB | 1.242 (0.656), build 99 s, +2.9 GB |

At the expected size (thousands to tens of thousands of distinct templates) exact search is 0.04 to 0.2 ms, so no index
is needed. At 1M vectors exact int8 with f32 rescore takes 8 ms at full recall, while HNSW is 10x faster but loses 23 to
34% recall and takes 96 s and 1.2 GB to build.

### Plan (decided by the coordinator 2026-10-28: after the current release order)

A shared crate **`paddock-embed`** (rule 42), used by Debrief's template search and Pacenotes' code search:

1. **Core** (pure, no I/O): `Embedder` port (`encode(&[&str]) -> Result<Vectors>`, `model_id`, `dim`); `StaticEmbedder`, the
   lean encoder with an f16 or int8 table chosen at load, a bounded rayon pool passed in (ER-20), no panics, texts cut at
   512 tokens; `VectorSet` (contiguous int8 or f32 rows plus ids) with exact top-k (8-lane dot, parallel over chunks, ties
   by id so results are deterministic) and int8 search with f32 rescoring (pays from about 100,000 vectors: 1.05 ms vs 1.84 ms single-threaded f32; at 10,000 f32 is as fast); `rrf(lists, k = 60)`.
   Model files checked by SHA-256; the index records model id, hash and dimension, and vectors of different models never mix.
2. **Debrief adapter**: embed **distinct templates** only (the mined or `@mt` templates and distinct messages of the
   catalogue), not rows: 505 events stood for 44k lines in the sample set. Vectors are cached in the store (a DuckDB
   table of template id, model id, int8 vector), built off the UI thread as a job with live progress and Cancel
   (ER-17/18), new vectors built beside the old and swapped. A search embeds the question (microseconds), takes the
   top templates, maps them to events through the existing template filter, and fuses with BM25 by RRF; the natural-
   language router (`debrief/nl-search`) sends identifier-like questions to names + BM25 (embeddings lost there in the
   October set) and plain-language questions to the fusion. Qwen3-Embedding through Pitwall is a second `Embedder` when
   Pitwall runs, with its own vectors.
3. **Index choice**: exact search (measured too slow nowhere in range: 1M vectors take 8 ms at full recall). Distinct templates are
   thousands to tens of thousands even for large log sets, where an exact int8 scan stays well under a
   millisecond (0.1 ms at 10,000, 1 ms at 100,000); HNSW adds a C++ build (usearch) or a large dependency for no gain at that size.
4. **Checks**: the 36-query set (and the nl-search evaluation set) as a regression test with a floor; a test that the
   encoder never panics on arbitrary bytes; licence clarify entry for model2vec's licence file if the crate is used at all
   (the lean encoder needs only `tokenizers`, `safetensors`, `half`).

## Results and conclusions

A/B on 10M generated events, 2 threads x 64 MB, release build with mimalloc, binaries interleaved (baseline `e8b60ea24`;
each step adds one change; head `eb8b0acb7`). Build docs/s and index size, then p50 ms.

| Step | Build docs/s | Index MB | Peak MB | `timeout` | `orderservice` | `pa` (prefix) | `t` (prefix) | `a3f` |
|---|---|---|---|---|---|---|---|---|
| baseline (mean of 2) | 143.8k, 168.5k | 537 | 309, 307 | 16.2 | 17.1 | 30.6 | 66.4 | 81.2 |
| + tokenizer | 203.1k | 537 | 310 | 15.1 | 16.3 | 30.3 | 66.3 | 107.4 |
| + id column, blocks | 217.3k | 537 | 336 | 1.24 | 2.53 | 16.6 | 51.9 | 67.9 |
| + prefix range | 217.8k | 537 | 313 | 1.10 | 2.29 | 16.1 | 52.9 | 65.5 |
| + no counts (layout 5) | 307.5k | 428 | 203 | 0.96 | 2.20 | 16.1 | 51.8 | 70.0 |
| head, two runs | 309.4k, 305.4k | 427 | 208, 202 | 0.90, 0.94 | 2.16 | 16.2, 16.5 | 51.3, 51.4 | 68.6, 68.7 |

(`timeout` here is the bare term; `all_ids` p50 is within noise at every step, e.g. `orderservice` 4.7 to 4.0 ms; `t` and
`pa` `all_ids` are over the cap.) The baseline pair differs by 17% (load 10.7 vs 8.2), the head pair by 1%. The
`+ tokenizer` row's slow prefix numbers (`conn` 45 ms, `a3f` 107 ms) were taken at load 13.9 and are noise. End to end
(2M events, whole import and index pipeline, two interleaved pairs): index 5.3 s and 6.4 s at baseline, 3.0 s and 3.2 s at
head (380k/312k to 672k/617k docs/s, about 1.9x); phrase total + 100 ids 54/51 ms to 12/12 ms; text total + 100 ids 5/4 ms
to 1/0 ms; the index on disk 63/67 MB to 45/55 MB; the import is unchanged (457k/808k vs 816k/774k events/s, the baseline
pair differs by 1.8x, so import is noise-limited). The cold `timeline 1000 buckets` query still misses its 500 ms budget at
both ends (863, 783, 809, 779 ms; not part of this work).

**Conclusion 1, writer budget:** 32 MB per thread, threads = `min(threads(WorkKind::Cpu), 8)`; 256 MB and 449k docs/s on
this machine (details and the fallback under [Writer budget](#writer-budget)). To be confirmed with a repeated, interleaved
run of the three cells named there before the default changes.

**Conclusion 2, no-counts layout: keep it.** Against the step before it (`+ prefix range`) it builds 41% faster (218k to
308k docs/s), the index is 20% smaller (537 to 428 MB, 53.7 to 42.8 bytes/doc), the build's peak footprint is 35% lower
(313 to 203 MB), and no query got slower (term 1.10 to 0.96 ms,
prefix 16.1 to 16.1 ms, `all_ids` unchanged). Nothing is scored, so counts and fieldnorms are never read. The cost is
the one-time rebuild of existing indexes (layout 5), which the build speed makes cheaper (10M docs in 32 s).

## Reproduction

The commands for the benchmark window are in `/Volumes/d7sst7/dev-targets/handoff/search-perf-bench.md`. In short:
`DEBRIEF_FTS_BENCH_DOCS=<n> DEBRIEF_FTS_BENCH_DIR=<work disk> <test binary> fts::bench --ignored --nocapture`, with
`DEBRIEF_FTS_BENCH_THREADS` / `DEBRIEF_FTS_BENCH_HEAP_MB` for the grid; `m2v-spike quality|encode|search` for Model2Vec.
