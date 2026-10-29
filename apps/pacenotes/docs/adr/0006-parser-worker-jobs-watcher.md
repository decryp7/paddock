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
| `pacenotes-parse-worker` | the worker program (`pacenotes-parse-worker`) and the wire types of its protocol. Depends on `pacenotes-parse`, `paddock-ipc`, `paddock-proc`; **not** on the index or SQLite |
| `pacenotes-engine` | the host side: the worker pool and `WorkerSource` (the index's `SymbolSource` over the pool), the stall judge, the index job on `paddock-jobs`, the watcher on `paddock-watch` |
| `pacenotes-index` | gains `Progress`, the pool, the size and mtime shortcut and the quarantine table; still no process, no parser |

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
3. **Cancel**: the call is cancelled through the IPC cancel, and if the worker does not end the call it is killed.

The probe of a worker's state is a port (`ProcessProbe`); the adapter reads `/proc/<pid>/stat` on Linux and asks `ps` through
`paddock_proc::run_bounded` on macOS. On Windows there is no adapter yet: a stuck worker is then ended only by Cancel or by its
memory cap, and the Windows adapter is a listed gap (the `paddock-proc` per-pid probe of ADR 0031 section 4 will provide it).
Starting a worker is waited for while the process is alive (the launcher fails at once when it exits); its connect window is a
cap of 10 minutes, not a liveness bound.

### The index job (ER-17 to ER-19)

`IndexJob` runs the indexer on the shared job pool (`paddock-jobs`), so the call returns at once and progress comes as job events.
Phases, each on its own scale, none restarting: **Finding files** (growing count, total unknown), **Reading files** (known total:
the files to fingerprint), **Reading code** (known total: the added and modified files), **Saving the index** (an
uncancellable step, declared, because the commit is the swap). Progress never goes backwards (a count only grows within a phase;
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

`paddock-watch` supplies the pieces: a `NotifySource` (OS notifications), a `Debouncer` on an injected clock (quiet 1 s, at most
5 s from the first event), and `spawn`. On a due event the engine starts an index run; runs never overlap (a request that arrives
during a run is coalesced into one more). Events inside `.git`, inside denied folders and for denied names are ignored before they
reach the debouncer. An event queue overflow or a watcher error triggers a full run, which is cheap with the shortcut. The watcher
stops with its handle; nothing is left running.

## Parallelism and cost (rule 51)

The expensive part is parsing, spread over `N` worker processes by the pool; the cheap path is not parsing at all, and not
reading: the shortcut. Per run: one `lstat` per file from the walk; a read and a hash only for files whose stamp changed; a
worker round trip only for files whose content changed. The worker count and the stall limits are Settings. Costs I expect to
matter and will measure: process start (hundreds of milliseconds each, paid once per worker and reused), the bytes copied to a
worker (the file, once), and the SQLite transaction (one per run).

## Consequences

- A quarantined file stays at tier 1 until it changes; the report names it, with the reason.
- Windows has no stall probe yet; documented, with Cancel and the memory cap as the backstops.
- The shortcut trusts mtime; a tool that preserves mtime while changing content defeats it (the same weakness as git's index);
  a "re-read everything" action in Settings is the answer.
