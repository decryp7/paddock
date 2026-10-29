# Performance record

Pacenotes' benchmarks, budgets and recorded results. How to run a benchmark reproducibly and the results template are in
the family guide, [docs/benchmarks.md](../../../docs/benchmarks.md). Record every run there quoted (newest first) here.
Pacenotes has no code in the tree yet. Its measurements come from the embedding research behind
[ADR 0001](adr/0001-embeddings.md).

## Benchmarks

| Benchmark | Measures | Corpus | Budget | CI |
|---|---|---|---|---|
| Embedding comparison ([ADR 0001](adr/0001-embeddings.md)) | Model load, chunks/s on 1 and 13 threads, peak RSS, binary size, retrieval quality (recall@5, hit@5) | 19,462 function chunks from 25 Paddock crates (10.9 MB, median 320 characters); 332 labelled queries (300 doc, 32 concept) | Quality bar: concept hit@5 ≥ 0.75 and doc recall@5 +25% over BM25 | No |

## Budgets

The [proposal](proposal.md) §21.4 sets memory targets to be measured: about 2 GB in total on a 16 GB machine, and 6–8 GB
on a 64 GB machine.

## Gaps

- The measurement plan on real repositories (re-rank latency, indexing time, peak memory and index size per 1M lines) is
  pending.
- The harness is not in the tree, so the numbers cannot be reproduced from `main`.

## Results (newest first)

### 2026-10-25 (amended 2026-10-28), embedding engines

- **Machine:** Mac mini M4 Pro, 14 cores, shared with other agents
- **Load:** given per number (1.9–42); one run per configuration
- **Build:** release (`lto = "thin"`, `strip`)
- **Corpus:** as above (synthetic in the sense that it is Paddock's own code, no user data)
- **Numbers:**

  | Engine | Model file | Load time | Chunks/s, 1 thread | Chunks/s, 13 threads | Peak RSS |
  |---|---|---|---|---|---|
  | potion-base-8M | 29 MB | 15 ms | 9,581 | 65,843 | 131 MB |
  | potion-code-16M-v2 | 32 MB | 34 ms | 6,365 | 55,499 | 200 MB |
  | potion-retrieval-32M | 125 MB | 44 ms | 8,001 | 57,239 | 357 MB |
  | BGE-small fp32 | 133 MB | 0.5–0.6 s | 17.1 | 14.3–15.8 | 2.6–3.0 GB (batch 1) |
  | Qwen3-Embedding-0.6B Q8_0, Metal | 610 MB | about 2 s | — | 29.6 (GPU) | 1.9 GB |

- **Quality, doc recall@5:** BM25 0.250; potion-code 0.330; BM25 + potion-code 0.320; BGE 0.447; Qwen 0.773.
- **Verdict:** BM25 + potion-code reaches 0.750 concept hit@5 and +28% doc recall, so it meets the bar.
- **Notes:** about 8 s per 1M lines with Model2Vec, against about 50 min with BGE. Source: [ADR 0001](adr/0001-embeddings.md).

## 2026-10-30, `pacenotes-git` open: the symbolic-link walk of the git folder

Rule 48/51; helper: `PN_WALK_REPO=<repo> cargo test -p pacenotes-git --test it -- --ignored --nocapture measure_open`.
Machine: the development Mac mini under normal load; one run each, second run shown where it differs. Repository: the Paddock
monorepo's main checkout (about 92,000 packed objects in 3 packs, 5,203 loose objects, many linked worktrees).

| Variant | `open()` time |
|---|---|
| walk everything under `.git` (first version) | 698 ms |
| walk only `objects/`, `refs/` and `info/`, refuse a link at any other top-level name, skip hooks/logs/worktrees/modules | 25 to 28 ms |

The folders skipped are never opened by gix. Debrief's checkout (2 packs, 3 loose objects), full walk: 143 ms. The cost is
dominated by loose-object files and linked worktrees; a repository with hundreds of thousands of loose objects would still
pay one `lstat` each, bounded by the 1,000,000-entry cap.
