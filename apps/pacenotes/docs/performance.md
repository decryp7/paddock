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

**The shortcut.** A second run over the unchanged tree of 20,596 files (2.3 GB on disk, build output included): 0.26 to 0.29 s, with 20,596 of 20,596 files not read (all shortcut hits),
against 9.8 to 20.6 s for the first run, so about 35x to 80x less. That is the cost of the folder watcher waking for an event that
changed nothing (an event inside `.git`, for instance).

**`.gitignore` walk** (second walk with the shared walker, `RepoFilesIgnore`; three rounds): the index's own walk without ignore
rules 220 to 230 ms (20,780 files; the first round 790 ms, cold cache); the shared walk 83 to 115 ms; the index's walk with the
ignore rules 120 to 128 ms (11,470 files). So honouring `.gitignore` costs about 205 ms against 225 ms without it, that is nothing
extra, and keeps 9,310 build-output files from being read and parsed.

**Cancellable "Finding files" (same day).** The shared `paddock_repo_files::walk` (the crate becomes `paddock-walk` after the shared-code freeze) cannot be cancelled and reports nothing until it
ends (7.5 s for 120,000 files on the reviewer's machine), so the engine runs it on its own thread and, meanwhile, a parallel
counting walk (`par_walk`, 4 workers) feeds the live count; a cancel returns at once and abandons the shared walk. On the 20,780-file
tree: shared walk alone 80 to 85 ms, shared walk plus counting walk 92 to 98 ms wall, that is 8 to 13 ms for the live count and the
cancel. Run it with `-p pacenotes-engine ... measure_ignore_walk`.

**Three walks per run.** A run walks the tree three times, and only the third is the index's own: (1) the shared
`paddock_repo_files::walk` for the `.gitignore` rules, single-threaded, on a thread of its own (80 to 85 ms on 20,780 files); (2) a
parallel counting walk beside it, for the live count and the cancel (about 8 to 13 ms more wall time; it ends before (1) does);
(3) the index's own walk, which applies the denylist, counts links and bounds depth and entries (110 to 130 ms with the ignore rules,
220 to 230 ms without them, because it then enters the 9,300 ignored files). Together about 200 to 230 ms on 20,780 files, against
220 to 230 ms for the one walk without ignore rules, so the three cost the same as one unfiltered walk and keep 9,300 files from
being read. At most one (1) runs at a time. Walks (1) and (2) become one, and walk (1) cancellable, when the shared walker is
unfrozen (ADR 0006).

## 2026-10-30, `pacenotes-rules` matcher (rule 48)

Helper: `cargo test --release -p pacenotes-rules --test it -- --ignored --nocapture measure_matcher`. Release build, the development Mac
mini at 5-minute load 16; one run. Corpus: a generated Rust file of 2,000 functions (161,780 bytes), invented code.

| Case | Time | Matches |
|---|---|---|
| typical rule `$X.unwrap()` | 25 ms | 2,000 |
| `$A + $B` | 23 ms | 2,000 |
| run placeholder `g($$$A)` | 25 ms | 2,000 |
| hostile: `f($$$A, $$$B, $$$C, $$$D, x)` against 40 arguments | 4.7 ms (ends by its step budget, no match) | 0 |
| hostile: 2,000-deep nesting | 2.4 ms | 0 |

So about 150 microseconds per kilobyte per rule, and each rule parses the file again (the parse is most of the 25 ms). With the
rules of a pack run one after another that is one parse per rule per file; sharing the index's parse tree across rules (take a `Tree`
in `find_all`) is the follow-up, measured when the engine wires the rules. The waiver glob is bounded by steps, not seconds: the
reviewer's case (`*a` fourteen times against 80 characters, which took over ten minutes) is checked as at most
`pattern length * path length * 2` steps in a unit test.

## 2026-10-30, blame and gix's similarity rename tracking (`Rewrites::default()`), parallelism and cost (ER-20, rule 48)

Helper: `cargo test --release -p pacenotes-git --test it -- --ignored --nocapture measure_blame`. Machine: the development Mac mini (Apple M4
Pro, 14 cores, 64 GB), 5-minute load 21 to 29 during the runs (other agents were building), release build, two runs per row. Repositories:
**invented and built in the scratch area** by the test (never a real repository): one file of 2,000 lines, 300 commits that each edit a
few lines of it; in the second repository the file is also renamed (`git mv`) every tenth commit (30 renames). The whole file is blamed
at the last commit.

| History | Rename tracking | Time | Hunks | Objects read |
|---|---|---|---|---|
| edits only | similarity (`Rewrites::default()`, what ships) | 278, 271 ms | 1,994 | 3,597 |
| edits only | none | 249, 274 ms | 1,994 | 3,597 |
| a rename every 10th commit | similarity (what ships) | 295, 264 ms | 1,994 | 3,747 |
| a rename every 10th commit | none | 1.2, 2.5 ms | **1** | 19 |

**Cheaper operation (ER-20).** Turning rename tracking off is not a saving: without it a renamed file's history ends at the rename, the
whole file goes to the commit that renamed it (one hunk, 19 objects, a millisecond), and the answer is wrong. With tracking on, the
extra cost over "edits only" is about 150 more objects (4%) and no measurable time at this size; it is the rename detection of the commits
that touch the file, not a search over the repository. So the default stays on, and the bounds that matter are the ones every blame
has: the object budget (default 2,000,000), the blob cap (2 MiB), `since`, and the line range. **Parallelism:** gix's blame walks one
history sequentially; the parallelism available is across files (one blame per file on a job pool), bounded by the object-database
reads (disk), a few at a time on an SSD, one or two on a hard disk; that belongs to the ownership view that calls it and is not
measured here. **Not measured:** a repository with hundreds of thousands of commits or very wide renames; the object budget stops those
with a limit error that says to set `since`.
