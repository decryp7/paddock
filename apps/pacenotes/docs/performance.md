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

## 2026-10-30, index run: parser pool, size and mtime shortcut, `.gitignore` walk, and a per-file wait found by measuring

Rule 48/51. Helpers (ignored tests, run by hand): `PN_MEASURE_REPO=<folder> cargo test --release -p pacenotes-worker --test it --
--ignored --nocapture measure_pool_and_shortcut` and the same with `-p pacenotes-engine ... measure_ignore_walk`. Machine: the
development Mac mini under normal load (5-minute load 15 to 22 during the runs), release build. Corpus: a Paddock worktree
(20,596 files without ignore rules: 6,550 parsed, 14,046 listed only, 184 over the size limit; no user data). One run per row.

**Bug found by this measurement.** The first pool run took 1,125 s for one worker (55 ms a file). Each parse call waited for its
monitor thread, which sampled the worker (a `ps` process on macOS) before looking at whether the call had ended, so every file cost a
`ps` run, and a beat (a second by default) whenever the call finished before the monitor first waited. Fixed: the monitor looks at
`done` first, takes no sample until its first beat and does not hold the lock while sampling; regression test
`a_call_that_finishes_does_not_wait_for_the_monitor` (a one-hour beat).

| Parse workers | First run (full index) | Speed-up |
|---|---|---|
| 1 | 20.6 s | 1.0x |
| 2 | 12.6 s | 1.6x |
| 4 | 9.8 s | 2.1x |

The read pass used 4 threads in all three. The gain flattens because the apply and SQLite steps are serial and the machine was
loaded; the Auto default (cores minus one, at most 8) stays, to be re-measured on an idle machine.

**The shortcut.** A second run over the unchanged tree: 0.26 to 0.29 s, with 20,596 of 20,596 files not read (all shortcut hits),
against 9.8 to 20.6 s for the first run, so about 35x to 80x less. That is the cost of the folder watcher waking for an event that
changed nothing (an event inside `.git`, for instance).

**`.gitignore` walk** (second walk with the shared walker, `RepoFilesIgnore`; three rounds): the index's own walk without ignore
rules 220 to 230 ms (20,780 files; the first round 790 ms, cold cache); the shared walk 83 to 115 ms; the index's walk with the
ignore rules 120 to 128 ms (11,470 files). So honouring `.gitignore` costs about 205 ms against 225 ms without it, that is nothing
extra, and keeps 9,310 build-output files from being read and parsed.
