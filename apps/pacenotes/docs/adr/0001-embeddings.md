# 1. Where Pacenotes' embeddings come from

Status: proposed (2026-10-25). The number is provisional; it is allocated again when this branch merges (see
`docs/suite-lessons.md`).

## Context

Pacenotes' semantic code search (proposal section 21, decision of 2026-10-23) needs an `Embedder` port: something that turns
code chunks and queries into vectors for the `VectorIndex`. Section 21.2 chose three implementations in this order: Pitwall
(Qwen3-Embedding), a small code model downloaded on a click and run through `candle` or ONNX Runtime, and none. This ADR
replaces the middle one after a measured comparison.

The user's priority for the choice (2026-10-25): **the lightest and fastest option that is good enough**. Weight (bundle
size, memory, native dependencies, start-up) and speed come first; quality has to meet a stated bar, not be the best.

Constraints that apply to every option:

- **Rule 23 (ad-hoc signing).** Every bundled Mach-O must carry the same Team ID. A vendor-signed dylib in the bundle is
  refused by dyld once library validation is on (Debrief 1.0.1: `libduckdb.dylib`, "different Team IDs"). An embedder must not
  bring a dylib of its own, or must be re-signed and verified like Debrief's DuckDB.
- **Licences** of code and weights must pass `deny.toml` (MIT, Apache-2.0, BSD and the other permissive entries).
- **Offline and local.** Nothing leaves the machine; the model is a file on disk, checked by hash.
- **Pitwall is optional** (AGENTS.md). Semantic search must still work, or say clearly what it needs, without it.
- **RAM** (proposal 21.4): Pitwall and language servers share the machine, so an embedder that holds gigabytes is a problem.
- **Re-indexing.** The index records the model id and dimension; vectors of different models never mix (section 21.2).

## Options compared

| | (a) Pitwall `/v1/embeddings` | (b) Model2Vec in process | (c) fastembed-rs in process |
|---|---|---|---|
| What runs | llama.cpp in Pitwall, Qwen3-Embedding-0.6B (GGUF Q8_0) | `model2vec-rs` 0.3.0: a token lookup table, mean pooling, no neural network at query time | `fastembed` 7.5.0 on ONNX Runtime 1.31 (`ort` 2.0.0-rc.15), BGE-small-en-v1.5 |
| Code licence | Pitwall (ours); llama.cpp MIT | MIT (`LICENSE` file; cargo-deny reads it, a `[[licenses.clarify]]` entry silences the warning) | Apache-2.0; `ort` MIT OR Apache-2.0; ONNX Runtime MIT |
| Weights licence | Apache-2.0 (Qwen/Qwen3-Embedding-0.6B-GGUF) | MIT (minishlab/potion-code-16M-v2, potion-retrieval-32M, potion-base-8M) | MIT (BAAI/bge-small-en-v1.5) |
| Native code | none in Pacenotes (HTTP via `paddock-pitwall`) | none beyond `libc++`/`libiconv` from the OS; pure Rust with the `fancy-regex` feature of `tokenizers` | ONNX Runtime linked statically (see below); C++ runtime, CoreML/Foundation frameworks on macOS |
| `cargo deny` (Pacenotes `deny.toml`) | n/a | licences, advisories, bans, sources ok; 102 crates | licences, advisories, bans, sources ok; 246 crates (with `hf-hub`) |

### Static linking of ONNX Runtime (option c)

Read from `ort-sys` 2.0.0-rc.15's build script and checked by building on macOS arm64:

- With `download-binaries` (fastembed's default), `ort-sys` downloads pyke's prebuilt **static** `libonnxruntime.a`
  (81 MB, hash-checked, from `cdn.pyke.io`) and links it with `rustc-link-lib=static=onnxruntime`. The built binary depends
  only on system libraries and frameworks (`otool -L`); there is **no vendor dylib**, so rule 23 is not at risk. The risk
  returns with `load-dynamic` or `copy-dylibs`, which must stay off.
- Prebuilt targets in this release (`dist.tsv`): `aarch64-apple-darwin`, `x86_64-pc-windows-msvc`, `aarch64-pc-windows-msvc`,
  `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`. **There is no `x86_64-apple-darwin` build**: Intel Macs would need a
  self-built ONNX Runtime.
- macOS: the prebuilt archive is compiled for macOS 13.4 and calls `__isPlatformVersionAtLeast`; the link failed with an
  undefined symbol until `libclang_rt.osx.a` was added to the link line (`-C link-arg=.../libclang_rt.osx.a`). A build script
  or `.cargo/config.toml` entry would carry this, and the app's minimum macOS becomes at least 13.4 for this crate.
- Windows (not built here): pyke's Windows libraries are always compiled with DirectML, so `ort-sys` links `DirectML`, `D3D12`,
  `DXGI`, `DXCORE` and `dxguid`. Whether the system copy of `DirectML.dll` is enough or the redistributable must be shipped is
  **unverified**.
- Linux (not built here): static as well, plus `libstdc++`.
- The build fetches a binary from a third-party CDN; CI needs network access or a cached copy, and Renovate must track the
  `ort` pin. Self-building instead (`ORT_LIB_PATH` pointing at an ONNX Runtime built from source with CMake and Python) avoids
  the CDN and covers Intel Macs, but adds a long C++ build per target to CI (not measured here; ONNX Runtime's own docs
  describe it as a multi-step CMake build).

## Measurements

**Machine:** Mac mini, Apple M4 Pro, 14 cores; release builds (`lto = "thin"`, `strip`). The machine was shared with other
agents. The 1-minute load is given next to each throughput number, as `docs/suite-lessons.md` asks. Spike code, corpus scripts
and labels were in `/Volumes/d7sst7/dev-targets/pacenotes-embed-spike` (build output removed afterwards).

**Corpus.** Pacenotes has no code or fixtures yet, so the corpus is public code: 25 crates from the local crates.io cache
(bytes, chrono, clap_builder, globset, hyper, ignore, image, indexmap, notify, regex-syntax, reqwest, rusqlite, semver,
serde_json, similar, tar, tempfile, time, tokio, toml_edit, tracing-subscriber, url, walkdir, zip, humantime), cut into
**19,462 function-level chunks** shaped like section 21.1's chunks: file path, doc comments, signature and body, at most 80
lines (median 320 characters, 10.9 MB). No user data.

**Labelled queries (332).**

- *Doc queries (300)*: the first sentence of a function's doc comment (at least six words and unique in the corpus), with that
  doc comment removed from the target chunk; one relevant chunk among 19,462. This is the CodeSearchNet-style task.
- *Concept queries (32)*: hand-written questions without identifiers ("detect symbolic link loops while walking a directory
  tree", "fail a future if it does not complete before a deadline"); the relevant set is every chunk whose path and function
  name match a pattern (1 to 40 chunks).

Metrics: **recall@5** (relevant chunks in the top 5, divided by min(5, relevant)), **hit@5** (share of queries with at least
one relevant chunk in the top 5) and **MRR@10**. BM25 (k1 1.2, b 0.75, identifiers split at camelCase and `_`) is the lexical
baseline; "BM25 + X" is reciprocal rank fusion (k = 60) of the two top-100 lists, as section 21.1 plans.

### Speed, memory and size

| | Model file | Binary | Load | Chunks/s, 1 thread | Chunks/s, 13 threads | Peak RSS (embedding the 19,462 chunks) |
|---|---|---|---|---|---|---|
| Model2Vec potion-base-8M (256 dims) | 29 MB | 3.6 MB | 15 ms | 9,581 (load 2.1) | 65,843 (load 2.1) | 131 MB |
| Model2Vec potion-code-16M-v2 (256 dims) | 32 MB (f16) | 3.6 MB | 34 ms | 6,365 (load 1.9); 5,526 on the first 2,000 chunks (load 5.3) | 55,499 (load 1.9) | 200 MB |
| Model2Vec potion-retrieval-32M (512 dims) | 125 MB (f32) | 3.6 MB | 44 ms | 8,001 (load 2.1) | 57,239 (load 2.1) | 357 MB |
| fastembed BGE-small-en-v1.5 (384 dims, fp32 ONNX) | 133 MB | 25.0 MB | 0.5-0.6 s | 17.1 on 2,000 chunks (load 6.5) | 14.3 on 2,000 chunks (load 21.6); 15.8 on all chunks (load 42) | 2.6-3.0 GB at batch 1; 11 GB at batch 8; 35-37 GB at batch 32 |
| fastembed BGE-small-en-v1.5 int8 (`BGESmallENV15Q`) | 34 MB | 25.0 MB | 4.8 s | | 47.3 on 2,000 chunks (load 22) | 0.86 GB at batch 1 |
| Pitwall path: llama.cpp b11669 + Qwen3-Embedding-0.6B Q8_0 (1024 dims), Metal | 610 MB | (Pitwall) | about 2 s | | 29.6 on all chunks, GPU | 1.9 GB (llama-server) |
| Same, CPU only (`-ngl 0 -t 13`) | 610 MB | | | | 11.6 on 200 chunks (load 13) | 2.2 GB |

For scale: the `hello` baseline with the same JSON and rayon plumbing is 0.46 MB, so Model2Vec adds about 3 MB and fastembed
about 24.5 MB to a binary. A clean release build with 2 jobs took 5 s for `hello`, 36 s for the Model2Vec program and 71 s
for the fastembed program (ONNX Runtime archive already cached; load about 8).

Notes on the numbers:

- RSS includes the 10.9 MB corpus and the output vectors held by the test program.
- fastembed's memory grows with every new input shape (ONNX Runtime's CPU arena with memory patterns; fastembed turns
  memory patterns off only for DirectML and exposes no switch), so length-sorted batches did not help. Intra-op threads did
  not speed up a model this small: 13 threads were no faster than one at batch 1 (the two runs had different machine load,
  6.5 and 21.6). Parallel speed would need several sessions, each with its own gigabytes.
- Model2Vec is about **350 times faster** than BGE-small on one core and needs a tenth of the memory. At these rates,
  1M lines of code (about 50k chunks, section 21.1) take about 8 s on one core with Model2Vec, about 50 minutes with
  BGE-small, about 28 minutes with Qwen3 on Metal and about 70 minutes with Qwen3 on the CPU.
- Qwen3-Embedding through llama.cpp on the CPU is about as fast as BGE-small through ONNX Runtime, at a much higher quality
  (next table): if a transformer embedder runs locally, llama.cpp with a GGUF model is the better engine, and that is what
  Pitwall already is.

### Retrieval quality

| Method | Doc queries recall@5 (= hit@5) | Doc MRR@10 | Concept recall@5 | Concept hit@5 | Concept MRR@10 |
|---|---|---|---|---|---|
| BM25 | 0.250 | 0.137 | 0.304 | 0.688 | 0.466 |
| Model2Vec potion-base-8M | 0.223 | 0.145 | 0.192 | 0.438 | 0.362 |
| Model2Vec potion-retrieval-32M | 0.237 | 0.137 | 0.196 | 0.500 | 0.376 |
| **Model2Vec potion-code-16M-v2** | **0.330** | 0.197 | 0.346 | 0.625 | 0.524 |
| **BM25 + potion-code-16M-v2** | 0.320 | 0.182 | **0.417** | **0.750** | **0.577** |
| BGE-small-en-v1.5 | 0.447 | 0.263 | 0.480 | 0.844 | 0.719 |
| BM25 + BGE-small | 0.353 | 0.206 | 0.445 | 0.844 | 0.537 |
| Qwen3-Embedding-0.6B (Pitwall) | **0.773** | **0.563** | **0.597** | **0.938** | **0.764** |
| BM25 + Qwen3-Embedding | 0.470 | 0.280 | 0.557 | 0.906 | 0.653 |

What the table says:

- **Qwen3-Embedding is in a different class**: 2.3 times Model2Vec's and 1.7 times BGE-small's recall@5 on doc queries.
- Of the Model2Vec models only the **code** model beats BM25; the general ones (base-8M, retrieval-32M) are worse than BM25 on
  code and are not worth shipping.
- Plain RRF with equal weights **hurts** when the dense side is much stronger than BM25 (BGE, Qwen3 on doc queries) and
  **helps** when both are weak (Model2Vec on concept queries). Fusion weights and the re-ranker (section 21.1) must be tuned
  on Pacenotes' own benchmark, not assumed.
- Limits: one language (Rust), public library code, 332 queries, one run per configuration; doc queries favour models trained
  on docstring-code pairs. The ordering is large enough to trust; differences of a few points are not.

### Quality bar

The vector leg is one of three searches and exists to widen recall for concept queries (section 21.1). It is **good
enough** when, fused with BM25, it (1) finds a right chunk in the top 5 for at least **3 of 4 concept queries**
(hit@5 >= 0.75), and (2) lifts doc-query recall@5 by at least **25%** over BM25 alone.

| | Concept hit@5 (bar 0.75) | Doc recall@5 vs BM25 (bar +25%) | Meets the bar | Weight |
|---|---|---|---|---|
| BM25 alone | 0.688 | | no | none |
| BM25 + potion-code-16M-v2 | 0.750 | +28% | **yes, just** | +3 MB binary, 32 MB model, 200 MB RSS, 6k chunks/s per core, pure Rust |
| BM25 + BGE-small | 0.844 | +41% | yes | +24.5 MB binary, 34-133 MB model, 0.9-37 GB RSS, 15-47 chunks/s, static C++ runtime, CDN at build time, no Intel Mac |
| Qwen3-Embedding (Pitwall), dense alone | 0.938 | +209% | yes, by far | Pitwall: 610 MB model, 1.9 GB RSS while loaded, 30 chunks/s on Metal |

## Decision (proposed)

The `Embedder` port gets three implementations, chosen in this order:

1. **Pitwall** when it is running and has an embedding model: `paddock-pitwall`'s `embed` (batches of 64) with
   Qwen3-Embedding-0.6B, at batch priority while Pitwall is idle (section 21.4). It is the quality option and the default
   whenever available.
2. **Model2Vec, built in**: `model2vec-rs` (pinned, `default-features = false`, feature `fancy-regex`, so no `hf-hub`, no
   network, no Oniguruma C build) with **potion-code-16M-v2** (MIT, 32 MB). It meets the bar in hybrid with BM25 and is the
   lightest option that does: no model server, no native runtime, start in milliseconds, a full re-embed of a large repository
   in seconds. The model file is either bundled or downloaded once on the user's click with a hash check; given its size,
   bundling is acceptable (decide at packaging time and record it here).
3. **None**: text and structural search only (unchanged, the default state until the user enables semantic search).

**fastembed / ONNX Runtime is not adopted.** BGE-small is better than Model2Vec but, on this machine, 350 times slower, 10 to
180 times larger in memory, and it brings a static C++ runtime, a CDN download at build time, a macOS link fix and no Intel
Mac build. Where better quality than Model2Vec is wanted, Qwen3-Embedding through Pitwall gives far more for the same
per-chunk time, without a second inference engine in Pacenotes. This replaces the "small code model through `candle` or ONNX
Runtime" of section 21.2 (`CodeRankEmbed`, `jina-embeddings-v2-base-code`). Revisit only if the Pacenotes benchmark on real
repositories shows Model2Vec failing the bar and users without Pitwall need more.

What we give up against the next heavier option (BGE-small): fused with BM25, 0.03 doc recall@5 and 0.09 concept hit@5 (dense
alone, 0.12 doc recall@5 and 0.22 concept hit@5), and English
prose understanding that a static model does not have (word order and negation are ignored). Against Pitwall: most of the
quality; the UI should say that semantic search is better with Pitwall and link to setting it up.

### Rules for the implementation

- **The index records the embedder**: engine (`pitwall` or `model2vec`), model id and revision (hash), dimension and the
  query/document prefixes. A different record means a full, resumable re-embed in the background with phase-labelled progress
  (rules 11 and 14); vectors of different embedders are never mixed and never compared. Switching from Model2Vec to Pitwall
  when Pitwall appears (or back when it goes away) is such a change: build the new index beside the old one, then swap.
- **Queries and documents** use the model's own instructions: Qwen3-Embedding queries carry
  `Instruct: Given a code search query, retrieve relevant code\nQuery: `; documents carry none; Model2Vec takes none.
- **Contain failures**: `model2vec-rs` panics on a tokenizer error (`expect("tokenization failed")` in
  `encode_with_args`), so encoding runs in the embed worker under the guard, never on a shared thread.
- **Limits**: inputs are truncated (512 tokens for Model2Vec, the chunker's cap for Pitwall); a missing or corrupt model file
  falls back to "no embeddings" with a message and a Repair action (rule 19).
- **Parallelism**: Model2Vec uses the CPU pool (cores - 1 or the Performance setting, rule 13); at 55-65k chunks/s on 13
  threads the vector index, not the embedder, is the bottleneck.
- **Benchmark**: the spike's harness (chunk corpus, doc and concept query sets, BM25 baseline, RRF) becomes a Pacenotes test
  fixture over generated sample repositories, so a model change is judged by the same numbers; it runs in the nightly
  benchmark workflow (rule 16).

## Consequences

- Semantic search works without Pitwall, offline, on every platform Pacenotes builds for, with one small pure-Rust
  dependency and one MIT model file.
- With Pitwall it is much better; the gap is large enough that the UI should say so.
- No ONNX Runtime, no CDN download in CI, no extra Mach-O in the bundle: rule 23 is untouched.
- Section 21.2 of the proposal and `docs/architecture-direction.md` change from "small local model downloaded on a click
  (candle or ONNX)" to "Model2Vec built in".
- The same engines were measured on Debrief's sample logs in Paddock's research note
  `docs/research/2026-10-embeddings-for-logs.md` (branch `docs/embeddings-for-logs`); Debrief keeps its no-vector-search decision.
- Open: whether int8 or smaller Model2Vec variants keep the quality (not measured), multilingual code comments (not tested;
  potion-code is English), and fusion weights for Model2Vec versus Qwen3 vectors.

## Sources (checked 2026-10-25)

- `model2vec-rs` 0.3.0 (crates.io, 2026-09-19; github.com/MinishLab/model2vec-rs, `LICENSE` MIT); models on Hugging Face:
  `minishlab/potion-code-16M-v2` (MIT, updated 2026-07-13), `potion-retrieval-32M` and `potion-base-8M` (MIT).
- `fastembed` 7.5.0 (crates.io, 2026-10-23, Apache-2.0; github.com/Anush008/fastembed-rs): pins `ort = "=2.0.0-rc.15"`;
  default features `onnx`, `ort-download-binaries-native-tls`, `hf-hub-native-tls`, `image-models`.
- `ort-sys` 2.0.0-rc.15 build script (`build/main.rs`, `build/static_link/mod.rs`, `build/download/dist.tsv`): ONNX Runtime
  1.31.0 prebuilt static libraries per target.
- `BAAI/bge-small-en-v1.5` (MIT) as served by fastembed from `Xenova/bge-small-en-v1.5` and `Qdrant/bge-small-en-v1.5-onnx-Q`.
- `Qwen/Qwen3-Embedding-0.6B-GGUF` (Apache-2.0), file `Qwen3-Embedding-0.6B-Q8_0.gguf`; llama.cpp `b11669`, the release pinned
  in Pitwall's `llama-runtimes.json`.
- Debrief `docs/releasing.md`, "Hardened runtime is off for ad-hoc builds" (the 1.0.1 Team ID failure).
