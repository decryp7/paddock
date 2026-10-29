# File finder (Dyno 1.0.0)

- Status: as built, 2026-10-29 (the jobs review, ER-23); the pages are Extras > Files
- What it does: lists the biggest files in a folder the person chose and the files with identical content. It **only
  reads** ([ADR 0021](../adr/0021-monitoring-only.md)): nothing is moved, removed or renamed.
- It is a job on the shared `paddock-jobs` (family ADR 0008): starting it answers at once, the shell's Jobs view and the page
  show its phases, "Still working" and Cancel; a cancelled or failed search changes nothing.

## Phases

| Phase | Scale | Cancel is checked |
|---|---|---|
| Listing files (`walk`) | a growing count of files found, no total | per entry, by every walker thread |
| Comparing the start of files (`heads`) | files that share a size (known after the walk) | per file and per 64 KiB |
| Comparing contents (`contents`) | bytes, out of the bytes of the files whose starts agree | per 64 KiB chunk |

A phase never restarts under its label and its count never goes back. The result is built on the side and handed over only
when the job ends well.

## Limits (Settings > Parallelism and cost)

The search is bounded; when it stops at a limit it says which one and what to do. The defaults, and the range the engine
validates each to (rule 51; the page only offers what the engine accepts):

| Setting (`disk.`) | Default | Range | When hit |
|---|---|---|---|
| `finderFiles` | 500,000 files and folders | 1,000 to 5,000,000 | the walk stops; the rest is not listed |
| `finderDepth` | 48 levels | 1 to 256 | deeper folders are left out |
| `finderHashGib` | 64 GiB | 1 to 4,096 | a file that would take the comparison past it is skipped; smaller ones after it are still compared |

Folders the walk could not read (no permission, a share that went away) are counted and named in the result sentence.

## Parallelism and cost

Rule 51 and ER-20: slow work gets more parallelism or a cheaper operation, bounded by the kind of work, at background
priority, with deterministic results.

- **Threads reading the folder** (`disk.readThreads`, shared with the disk map's walk): **Auto** or 1 to 16. Auto follows the
  kind of volume: 4 to 8 threads on an SSD (cores minus one, at least 4), **1** on a spinning disk (parallel seeks make it
  slower), **2** per server on the network. A volume whose kind is unknown is treated as an SSD. A fixed number replaces the
  table (held to 1 to 16 by the engine).
- **Jobs at once** (`jobs.workers`): **Auto** (4) or 1 to 8. The pool is built when Dyno starts, so a change takes effect at the
  next start; the rest of the jobs wait as `queued`.
- The walk is `par_walk` (directories are the unit of work); the heads and the contents are hashed with `par_map_ordered`. Results
  come back in input order, so the answer does not depend on the thread count or on scheduling (a unit test reads the same tree
  with 1 and with 8 threads and compares the two answers).
- One `stat` per file gives its size and its modification time (`WalkOptions::with_size` and `with_modified`); nothing is read
  twice. Only files that share a size are hashed, first their first 64 KiB, then the whole of those whose starts agree.
- **Cost:** memory is the file list (a path, a size and a time per file at least the size floor; at most 500,000 by default),
  CPU is the hashing (one core per thread while it runs), I/O is one read of the folder tree and the bytes of the candidate
  duplicates (at most `finderHashGib`). Nothing is kept after the result is handed over except the 200 largest files and 100
  duplicate groups the page shows.
- **Measured** (rule 48): [performance.md](../performance.md#file-finder-2026-10-29). 4 threads take about a third of the
  sequential time on a warm 50,000-file tree. Not measured: a spinning disk and a network share.

## A folder that does not answer

A dead network share can hold a `read_dir` or a `stat` inside the kernel for as long as the server is gone. The first look at
the chosen folder gives up after 5 s with a sentence. The walk runs on a thread Cancel can abandon: Cancel returns at once, and
the abandoned search's threads count on their host in the disk map's own per-host count until the calls return. A host with
four such threads gets no new search or scan ("Restarting Dyno clears it"). The disk map's walk and the finder count together,
so one dead share cannot starve either.

## Not done

- The Windows Event Log is read by `wevtutil` in one step: its output is parsed after the tool ends, so that read shows a
  phase without a count and Cancel takes effect when `wevtutil` has answered (the macOS unified log streams and is killed at
  once). Streaming `wevtutil qe /f:xml` is possible and is a gap, not a decision; see [roadmap](../roadmap.md).
