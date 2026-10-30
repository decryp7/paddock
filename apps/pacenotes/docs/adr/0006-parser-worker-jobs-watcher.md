# 6. The parser worker, the index job, the pool and the watcher

Status: proposed (2026-10-30), design note of package A3 (plan section 4.4). Numbered at merge. Builds on ADR 0003 (the grammar
library is not crash-isolated), ADR 0005 (the pure half of the index) and ADR 0031 (patience, not wall clocks).

## Context

A3a left the index with two ports and no process: the parser runs nowhere yet. A3 puts parsing in its own process so a crash in a
grammar's C scanner (an abort, a stack overflow, an infinite loop) costs one file and not the app, runs the index as a job with
live phases and Cancel (ER-17 to ER-19), spreads the per-file work over a bounded pool, skips reading files that did not change,
and watches the workspace so the index follows edits.

## Decision

### Crates

| Crate | What it is |
|---|---|
| `pacenotes-worker` | the worker program (`pacenotes-parse-worker`) and the wire types of its protocol, and the host side: the worker pool and `ParseWorkers` (the index's `SymbolSource` over the pool), the stall judge and the process probe. Depends on `pacenotes-parse`, `pacenotes-index`, `paddock-ipc`, `paddock-proc`; the program itself never opens SQLite |
| `pacenotes-engine` | the edge that joins the index to the shared crates: `JobProgress` (the job reporter as the index's `Progress`), `RepoFilesIgnore` (`.gitignore` from `paddock-repo-files`), `IndexService` (the index job on `paddock-jobs`) and its folder watcher on `paddock-watch` |
| `pacenotes-index` | gains `Progress`, `Ignore`, the pool, the size and mtime shortcut and the quarantine table; still no process, no parser |

### The worker and its protocol

One request, one reply, on the family IPC (`paddock-ipc`: private endpoint, per-launch token, length-prefixed postcard frames).
`ParseRequest { path, bytes }` (at most the file limit, 2 MiB) and `ParseReply { facts }` where `facts` is `None` for a file in no
known language. The worker holds no file-system access beyond its own binary: the host reads the file (through the denylist and
the workspace checks) and sends the bytes, so a hostile file can at worst crash the worker. The worker runs the parser with the
host's `Stop` being the call's cancel probe, so a cancelled call stops the parse. It answers `--self-test` (rule 23) and exits
when its stdin closes. It starts at lower priority, with a clean environment and a memory cap (default 1 GiB, a Setting).

### Crash and stall policy (ADR 0031: no wall clocks)

The host never puts a deadline on a parse. Three things end a call early, none of them elapsed time:

1. **The worker died** (abort, signal, memory cap, a stack overflow in C): the call returns a closed-connection error. The file is
   **quarantined**: the failure is recorded against the path and the file's fingerprint in the index (`quarantine` table), the
   rest of the run continues with a fresh worker, and the file is not sent again until its content changes. There is no retry:
   a parser crash is deterministic for a given input, and a retry loop is how a run hangs.
2. **The worker is stuck** (a loop in a scanner): judged by `StallJudge`, ADR 0031's rule as a pure state machine over per-tick
   observations of the worker process. A *tick* is one wake-up of the host's monitor thread (a beat of 1 s, but the unit is the
   wake-up, never the seconds: a host that was paused for an hour sees one tick). Per tick the judge is given the worker's run
   state, whether its CPU time grew, and whether the call made progress (a parse call reports none, so a call is "without
   progress" throughout). It counts **blocked ticks** while the worker is blocked and owes nothing, and **spin CPU** while the
   worker uses CPU without progress. A stopped worker (paused by the OS), a starved one (runnable, no CPU), or one waiting on
   I/O counts nothing. Limits are Settings with defaults: 120 blocked ticks and 60 CPU-seconds. A stuck worker is killed; the
   file is quarantined as in 1.
3. **Cancel**: the worker is killed. A parse call holds no state worth keeping (its result is a set of facts that nothing has
   stored yet), so there is nothing to wind down: the pool ends the process, drops the call and starts a fresh worker for the next
   file. The worker would also stop a call on an IPC cancel (the parser checks the call's token), but the host does not send one and
   wait. The call's token wakes the monitor through `on_cancel` (not at its next beat), and the monitor kills the worker; killing
   cannot hang on a worker that is stuck.

The probe of a worker's state is a port (`ProcessProbe`); the adapter reads `/proc/<pid>/stat` on Linux and asks `ps` through
`paddock_proc::run_bounded` on macOS. **Windows has no adapter, and falls back on purpose.** `ProcessProbe::sample` returns `None`, which the judge reads as "no
observation": it counts nothing, so a worker stuck in a loop on Windows is ended only by Cancel (which kills it) or by its memory
cap, never by the stall judge. Reasons: (1) a per-process "blocked or spinning" reading on Windows needs thread-level state and
CPU times through `NtQuerySystemInformation` or the toolhelp and `GetProcessTimes` calls, which are `unsafe` FFI that the shared
`paddock-proc` is meant to own (ADR 0031 section 4), not this app (shared crates are frozen for this work, and Pacenotes does not
add `unsafe`); (2) it cannot be built or tested on the machine that wrote this, and an untested process probe would be worse than
an honest gap; (3) the loop-in-a-scanner bug it guards against has not been seen outside crafted input, and the first Windows
release lists the gap. When `paddock-proc` gains the per-pid probe, the adapter is one new `ProcessProbe` implementation.
Starting a worker is waited for while the process is alive (the launcher fails at once when it exits); its connect window is a
cap of 10 minutes, not a liveness bound.

### The index job (ER-17 to ER-19)

`IndexService::start` runs the indexer on the shared job pool (`paddock-jobs`), so the call returns at once and progress comes as job events.
Phases, each on its own scale, none restarting (`JobProgress` ignores a repeat of the phase it is in, and raises a total whenever the
count passes it, so a bar never shows more done than the whole): **Scanning the folder** (growing count of the files found, total
the same count at the end; it starts before the first file is looked at and covers the `.gitignore` walk, see below), **Finding
files** (the index's own walk, with its own count), **Reading files** (known total: the
files to fingerprint), **Reading code** (known total: the added and modified files), **Saving the index** (an uncancellable step,
declared with `uncancellable` and lifted at the next phase, because the commit is the swap). A job cancelled while it is still
queued never starts. Progress never goes backwards (a count only grows within a phase;
a new phase is a new scale). Cancel works in every phase except the declared one and leaves the previous generation readable,
because nothing is written before the commit. The UI never times a job out ("Still working").

### Pool and the shortcut (rule 51, ER-20)

Both per-file passes run on `par_map_ordered` with a worker count that is a Setting with an "Auto" default (cores minus one,
at most 8 for the parse pass because each worker is a process with its own memory; fingerprinting is bounded by the disk, 4 on
an SSD). Results are applied in path order, so a run is deterministic whatever the scheduling. **The shortcut:** the store keeps
each file's size and modification time and when it was read; a file whose size and mtime match and whose mtime is at least two
seconds older than the read is **not read again** (git's "racy" rule, so an edit within the timestamp granularity is never
missed). The effect is measured in `docs/performance.md`.

### The watcher

`IndexService::watch` joins the pieces of `paddock-watch`: a `ChangeSource` (the app passes `NotifySource`, OS notifications), a
`Debouncer` on an injected clock (quiet 1 s, at most 5 s from the first event; `WatchTiming`) and `spawn`. When the debouncer says
the folder is quiet, the watcher's thread starts an index run **and waits for it**, so runs never overlap; changes that arrive
meanwhile queue on the channel and become one more run afterwards (the plan reads the folder as it is then, so none is lost). The
watcher stops with its handle; a run in flight finishes first.

Known limits, recorded rather than worked around, because the shared watcher is frozen: `ChangeSource` reports *that* something
changed, not *what*, so events inside `.git`, inside denied folders and for ignored files all start a run; such a run is cheap
(a walk and a `lstat` per file, no reads, thanks to the shortcut) and finds nothing to do. The shared `NotifySource` drops watcher
errors and queue overflows instead of reporting them, so the engine cannot yet turn them into a full run; the Settings action
"Re-index everything" and the next quiet change are the recovery. Both go away when `paddock-watch` carries paths and errors.

### `.gitignore` (ER-22)

The index must not read or parse build output, vendored trees or anything else the project ignores, and the rules are git's
(nested `.gitignore` files, negations, anchoring), which `paddock-repo-files::walk` already applies through the `ignore` crate
(also without a git repository, `.git` skipped). So Pacenotes does not copy a matcher: the index has an `Ignore` port (visible
file, visible folder) and `RepoFilesIgnore` answers it from one `paddock_repo_files::walk` of the workspace. A file is visible when
the shared walk lists it; a folder when a visible file lies under it, so an ignored folder is not entered by the index's own walk
at all. Why not walk once with the shared crate and skip the index's own: the index's walk also applies the denylist, counts links
and denied entries, bounds depth and entries and is cancellable per directory, none of which the shared walk does. The price is a
second walk of the tree (measured in `docs/performance.md`; it is repaid by the files it keeps out).

**Cancel and progress in this phase.** The shared walk takes no cancel token and reports no count, and on a large tree it runs for
seconds (7.5 s for 120,000 files). `RepoFilesIgnore::scan_cancellable` therefore runs it on a thread of its own and, on the job
thread, runs `paddock_jobs::par_walk` (cancellable, parallel) as a counting walk that feeds "Finding files" with the files found so
far and ends with the total. The wait for the shared walk is woken by the job's cancel token (`on_cancel`), not by ticks. **At most one shared walk runs at a
time**, an abandoned one included (`WalkSlot`): a scan that starts while one still runs waits for the slot, and the wait ends at once on
cancel, so cancelling and starting again cannot pile up walks of a very large tree. **On cancel the shared walk is abandoned**: the job returns Cancelled at once, the walk finishes by itself in the background holding only its
own data, and its result is dropped. The count ignores `.gitignore` (an upper bound) and costs one extra parallel walk, 8 to 13 ms
on a 20,000-file tree. **TODO (after the shared-code freeze):** give `paddock_repo_files::walk` a cancel token and a progress
counter, and make this one walk again; the abandoned thread and the counting walk then go away.

**Reuse of the walk (rule 51), not done.** Skipping the shared walk when no `.gitignore` changed cannot be done safely here: a new
file still has to be judged against the ignore rules, and the rules are only reachable through the walk (the shared crate has no
matcher API). **Three walks per run** (see `docs/performance.md`): the shared `.gitignore` walk, the counting walk beside it, and the index's own walk. Cost per run on a 20,000-file tree is about 85 ms, and the index's own walk is the dearer half. A cached matcher needs
the same shared change as the TODO above.

**The index file is never inside the repository.** `IndexService::start` and `watch` refuse a `store_path` whose canonical parent
lies inside the canonical root (`EngineError::StoreInsideWorkspace`, with the message "keep the index outside the repository"):
the file would be one of the repository's own files, every run would change it and the watcher would never stop. The check fails
closed: a repository folder or index place that cannot be resolved is refused too (`RootUnresolvable`, `StoreUnresolvable`), never
let through. **The denylist is checked
first and still wins**: a secret that `.gitignore` also lists is counted as withheld, never merely ignored, and an `!allow` line in
`.gitignore` cannot make a denied file readable. The shared walk stops with an error at its file cap; the run then fails with that
message instead of indexing a partial set.

**Probing workers: one `ps` for all, not done.** Batching the macOS `ps` into one call per tick for every worker would need one
sampler thread shared by all monitors and a cache of its answers. The measurement that asked for it was the per-file `ps`, which is
gone: a monitor takes no sample before its first beat (one second), so a file that parses in under a second, which is nearly every
file, costs no `ps` at all. What remains is one `ps` per beat per call that runs longer than a beat. Batching is worth doing when a
measurement shows many slow calls at once.

## Parallelism and cost (rule 51)

The expensive part is parsing, spread over `N` worker processes by the pool; the cheap path is not parsing at all, and not
reading: the shortcut. Per run: one `lstat` per file from the walk; a read and a hash only for files whose stamp changed; a
worker round trip only for files whose content changed. The worker count and the stall limits are Settings. Costs I expect to
matter and will measure: process start (hundreds of milliseconds each, paid once per worker and reused), the bytes copied to a
worker (the file, once), and the SQLite transaction (one per run).

## Consequences

- A quarantined file stays at tier 1 until it changes; the report names it, with the reason.
- Windows has no stall probe (see above); Cancel and the memory cap are the backstops.
- The shortcut trusts mtime; a tool that preserves mtime while changing content defeats it (the same weakness as git's index);
  a "re-read everything" action in Settings is the answer.
