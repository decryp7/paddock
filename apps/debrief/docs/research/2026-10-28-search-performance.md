# Search performance: Tantivy and Model2Vec (2026-10-28)

Question (user): "improve tantivy and model2vec for best performance". Method: measure first, profile, then change only
what the numbers support. Status: **partly measured.** The machine's load stayed between 14 and 220 all session (other
agents' builds and tests), so the timed before/after runs are scheduled for an unattended window; the commands are in
[Reproduction](#reproduction). Numbers marked *(load n)* were taken under that load and are indications only.

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
| `e9a65e6b9` | `words_ref`: ASCII text lower-cased once, words handed out as slices, no `String` per word; other text takes the old path (`words_general`), so index and oracle still share one implementation | proptests (2,256 cases) and fixed cases: ASCII path = general path | window |
| `e9a65e6b9` | Tokenizer reuses its tokens between texts (at most 4,096 kept) | unit test | window |
| `e9a65e6b9` | Prefix = term range `[p, p + U+10FFFF)` instead of a regex compiled per search | test: same hits as `p.*` on ASCII and accented words | window |
| `e9a65e6b9` | `search` opens the id column once per segment, not per hit; `all_ids` reads blocks of 64 hits (`collect_block`, `first_vals`) | search and differential integration tests | window |
| `69b35c28a` | Words without counts or fieldnorms, `source`/`seg` without fieldnorms (nothing is scored); layout 5, existing indexes rebuild once | unit and integration tests | **pending** |
| `dcb70b699` | The `debrief-fts` thread runs at background priority; Tantivy's indexing and merge threads start from it and inherit it (macOS QoS, Linux nice) | | |

Not changed, with reason: the merge policy (final merges were 1.5 s per 1M docs and 5 segments remained; revisit only if the
10M run shows many segments slowing prefixes); mmap vs RAM directory (searches are already sub-millisecond on mmap and the
OS cache, and a RAM copy would add the index to the process's real memory); the collector (`TopDocs` by the `ts` fast
field is what the grid needs); the doc store codec (empty). Dropping the `lz4-compression` feature would only shrink the
binary; not worth a change by itself.

### Writer budget (pending the grid)

Tokenizing runs inside Tantivy's indexing threads, so build speed should scale with writer threads until the feeder or
Parquet reading limits it. Tantivy needs at least 15 MB per thread; ADR 0014's budget is 64 MB. The proposal waits for the
grid (2/4/8/13 threads × 64/128/256 MB, docs/s and peak footprint at 10M docs). The default will come from
`debrief_core::threads::threads(WorkKind::Cpu)` (cores − 1, ER-20) bounded by a per-thread heap, not a new constant.

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

Pending in the window: encode throughput (model2vec-rs vs lean f32/f16/int8, 1 and 13 threads, share of tokenizing),
load time and footprint; exact top-100 search (naive, 8-lane, parallel, int8, int8 + f32 rescore) vs usearch HNSW (f32,
i8) and hnsw_rs at 1k, 10k, 100k and 1M vectors of 256 dimensions (latency, recall@10, build time, memory).

### Plan (decided by the coordinator 2026-10-28: after the current release order)

A shared crate **`paddock-embed`** (rule 42), used by Debrief's template search and Pacenotes' code search:

1. **Core** (pure, no I/O): `Embedder` port (`encode(&[&str]) -> Result<Vectors>`, `model_id`, `dim`); `StaticEmbedder`, the
   lean encoder with an f16 or int8 table chosen at load, a bounded rayon pool passed in (ER-20), no panics, texts cut at
   512 tokens; `VectorSet` (contiguous int8 or f32 rows plus ids) with exact top-k (8-lane dot, parallel over chunks, ties
   by id so results are deterministic) and int8 search with f32 rescoring if the window shows it pays; `rrf(lists, k = 60)`.
   Model files checked by SHA-256; the index records model id, hash and dimension, and vectors of different models never mix.
2. **Debrief adapter**: embed **distinct templates** only (the mined or `@mt` templates and distinct messages of the
   catalogue), not rows: 505 events stood for 44k lines in the sample set. Vectors are cached in the store (a DuckDB
   table of template id, model id, int8 vector), built off the UI thread as a job with live progress and Cancel
   (ER-17/18), new vectors built beside the old and swapped. A search embeds the question (microseconds), takes the
   top templates, maps them to events through the existing template filter, and fuses with BM25 by RRF; the natural-
   language router (`debrief/nl-search`) sends identifier-like questions to names + BM25 (embeddings lost there in the
   October set) and plain-language questions to the fusion. Qwen3-Embedding through Pitwall is a second `Embedder` when
   Pitwall runs, with its own vectors.
3. **Index choice**: exact search unless the window shows it too slow at the expected sizes. Distinct templates are
   thousands to tens of thousands even for large log sets, where an exact int8 scan is expected to stay well under a
   millisecond (pending measurement); HNSW adds a C++ build (usearch) or a large dependency for no gain at that size.
4. **Checks**: the 36-query set (and the nl-search evaluation set) as a regression test with a floor; a test that the
   encoder never panics on arbitrary bytes; licence clarify entry for model2vec's licence file if the crate is used at all
   (the lean encoder needs only `tokenizers`, `safetensors`, `half`).

## Reproduction

The commands for the benchmark window are in `/Volumes/d7sst7/dev-targets/handoff/search-perf-bench.md`. In short:
`DEBRIEF_FTS_BENCH_DOCS=<n> DEBRIEF_FTS_BENCH_DIR=<work disk> <test binary> fts::bench --ignored --nocapture`, with
`DEBRIEF_FTS_BENCH_THREADS` / `DEBRIEF_FTS_BENCH_HEAP_MB` for the grid; `m2v-spike quality|encode|search` for Model2Vec.
