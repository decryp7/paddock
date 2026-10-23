# 8. Long operations: phases, progress, cancel and parallel work

- Status: accepted (2026-10-23, user decision "ADR + shared crate first")
- Related: ADR 0006 (consumption, versioning), `docs/audits/2026-10-23-long-operations.md` (the findings this fixes),
  crate `paddock-jobs`, component `pd-job` in `@decryp7/paddock-ui`

## Context

The audit found the same defects in every app: progress without phases (Debrief `ProgressSnapshot`/`JobProgress`), bars that restart
under one label (download, then checksum), silent tails (compaction, indexing, archive writing), operations with no job, no progress
and no Cancel at all (Pitcrew, Spotter, Dyno, Debrief dumps and Chromium install), fixed UI timeouts that kill healthy long jobs,
a second read pass to hash what was just written, and sequential loops over trees and sources. Rules (user, 2026-10-23): live
phase-labelled progress from the first moment, one scale per phase, Cancel in every phase, bounded multi-threaded and pipelined work
with deterministic results, and every operation off the UI/main/IPC-handler threads. Built once and shared, not five times.

## Decision

One Rust crate, `paddock-jobs` (no UI or Tauri dependency; apps wire one 10-line event sink) and one Angular component, `pd-job`.

### 1. Job model (wire contract, camelCase JSON, event `paddock-job`)

A **snapshot** is the whole job as of `seq`; events are snapshots, so they are idempotent, coalescable and safe to drop. The UI keeps
the highest `seq` per `id` and can ask for `snapshots()` when a pane opens.

```
JobSnapshot { id, kind, title, subject?, state, seq, elapsedMs, etaMs?, cancellable, notCancellableReason? }
state = { state: "queued" }
      | { state: "running", phase?: { id, label, unit: "items"|"bytes", total?: u64, done: u64 } }
      | { state: "done" } | { state: "failed", message } | { state: "cancelled", reason: "user"|"deadline" }
```

- State machine: `queued -> running(phase)* -> done | failed | cancelled`; a queued job may be cancelled. Terminal is final and
  nothing is emitted after it. Illegal states are unrepresentable (only a running job has a phase, only a failed one a message).
- `kind` is an app-defined slug ("import", "download", "scan"); `subject` is a display string, never parsed; ids are numbers.
- **Phases own their scale.** Starting a phase resets `done` to 0 and is emitted at once; a stage never restarts under the same label
  (download, verify, unpack are three phases). `unit` is items or bytes. `total` unknown means **indeterminate with growing count**:
  the bar is indeterminate and the label shows "Finding files... 12,345 found"; `set_total` switches it to determinate when known.
- **ETA rules** (`EtaEstimator`): never for an unknown total; not before 2 s of work and 3 % done; exponentially smoothed rate
  (0.3); `0` when done; hidden past 99 h or when the rate is zero. The UI shows ETA only when `etaMs` is present.

### 2. Throttled events

At most one event per job per 100 ms (`DEFAULT_MIN_INTERVAL_MS`, ~10/s); the newest value wins (coalescing); phase and state changes
bypass the throttle; `flush()` publishes a pending value (called by `JobContext::run` before the terminal event). `add` is lock-free
on the fast path, so hot loops on many threads may call it per item. Sinks run under the reporter lock (events arrive in `seq` order),
must be quick, and a panicking sink is contained and dropped. The Tauri adapter is
`impl EventSink for Emit { fn emit(&self, s: &JobSnapshot) { let _ = app.emit(EVENT_NAME, s); } }`; no `tauri` feature in the core.

### 3. Cancel: cooperative, with a guarantee

`CancelToken` (clones share state; first reason wins: `user` or `deadline`). Every helper in the crate checks it at least every
`CANCEL_CHECK_ITEMS` (256) entries or every `CHUNK` (64 KiB) of I/O, so **cancel stops within `CANCEL_LATENCY_TARGET_MS` (250 ms)
plus the duration of one item of caller code** (tests assert this for walk, map, pipeline). A step that cannot be interrupted
(database checkpoint, atomic rename, a library call) must say so: `JobContext::uncancellable("Writing the database")` sets
`cancellable: false` and `notCancellableReason`, which the UI shows instead of an enabled Cancel; the guard restores it on drop.
Silent un-cancellable stretches are a defect. Cancelling a queued job ends it at once; a job cancelled while queued never starts.

### 4. Deadlines and UI timeouts

Deadlines belong to the engine: `register_with_deadline` / `CancelToken::with_deadline` cancel with reason `deadline` ("time limit
reached"), and the job ends `cancelled` with that reason. **The UI never times out a job.** Commands that start work return at once
(the job id); the UI subscribes to events and, when no event arrived for N seconds while `state` is running, shows "Still working"
(not an error). The per-command fixed UI timeouts found in the audit (Pitcrew `call-timeout.ts`) are removed for job-backed commands.

### 5. Streaming hash while writing

`HashingWriter` (SHA-256, counts bytes, optional `Counter` and `CancelToken`) and `copy_hashed` hash in the write path. A download is
one pass: bytes go to the temp file and the hasher together, the digest is compared before the rename, and the verify step is a
named instant phase, never a second read of the file. Archives extracted by us hash while extracting. Expected-checksum fetches may
run in parallel with the download.

### 6. Parallel helpers (all bounded, deterministic, cancellable, panic-contained)

- `Workers` (1..=64; `auto()` = cores - 1, at most 16): the only knob; apps pass their CPU limit, the crate reads no globals.
- `par_walk(root, WalkOptions, cancel, found)`: directory-granular parallel walk with a live found-count (`Counter`), an ignore-rules
  hook (`Include | Skip | SkipSubtree`), a symlink policy (`Ignore | Report` default `| Follow` with canonical-path loop cut),
  max depth, optional sizes, unreadable entries collected as issues (capped) instead of aborting. The result is sorted by path, so it
  is identical for any worker count.
- `par_map_ordered(items, workers, cancel, counter, f)`: results in input order; the error of the lowest failing index wins.
- `pipeline_ordered(source, workers, window, cancel, work, sink)`: the source is pulled on its own thread (discovery continues while
  earlier items are processed), `work` runs on a bounded pool, `sink` receives results in order on the caller's thread; in-flight
  items are bounded by `window` (the bounded channels), so one slow item cannot grow memory.
- Panics in `f`, sources, filters and sinks become `JobError::Panicked` attributed to the job; the process and pool survive.
- Memory: helpers hold at most `window` items (pipeline) or the input vector (map); big inputs use the pipeline.

### 7. Everything off the UI thread

Rule: **no operation that can take noticeable time runs on the UI thread, the Tauri main thread or an IPC handler's thread.** A
command registers the job, submits it and returns the id; progress and result arrive as events.

- `JobPool::new(Workers)`: bounded worker threads; `spawn_job(ctx, body)` returns at once (the job is `queued` until a worker is free,
  which is the app's concurrency limit); `shutdown`/drop drains the queue.
- `JobPool::offload(f)`: a runtime-agnostic future that runs a blocking closure on the pool, for async commands (Tauri async
  commands, tokio) so the runtime's threads never block; panics come back as `JobError::Panicked`.
- Detection: `assert_nonblocking(budget, || handler(...))` (budget `HANDLER_BUDGET_MS` = 50) is the test helper every command test uses;
  a handler that does I/O or heavy work inline fails with a message pointing to the pool. A static lint is not feasible in stable
  clippy (blocking is a runtime property); each app adds this assertion to the test of every command and a grep-based CI check
  (`std::fs`, `std::thread::sleep`, `reqwest::blocking`, `Command::output` inside `#[tauri::command]` bodies) as a second net.
- Use multiple threads wherever work can be split (walk, hash, parse, redact, compress, compact); the helpers above are the means.

### 8. Angular

`<pd-job [job]="snapshot" (cancel)="...">` (`SuJob`) in `@decryp7/paddock-ui` renders title and subject, the phase label with a
`step` text, the count ("12,345 found", "42 MB of 118 MB", "3 of 40"), a determinate bar (`role="progressbar"` with
`aria-valuenow/min/max`) or an indeterminate bar with the growing count, ETA only when present, a Cancel button (hidden for terminal
states; disabled with the reason as text when `cancellable` is false), terminal states with word and icon (Done, Failed, Cancelled
with reason), a "Still working" hint when `stalled`, and a polite `aria-live` region that announces phase changes and completion (not
every tick). Pure helpers (`jobView`, `formatCount`, `formatBytes`, `formatEta`) are exported and unit tested. Apps keep their
Jobs store; the component is presentational (the formatting is display only; all logic is in Rust).

### 9. Consumption

Per ADR 0006: git tag dependency now (`paddock-jobs = { git = ".../paddock", tag = "v0.x.y" }`), Gitea cargo registry later; publishing only
from the tag workflow. Crate `version` starts at 0.1.0; pre-1.0 a minor bump is breaking (the wire model counts as API; adding an
optional field is a patch). `crates/deny.toml` allows permissive licences only; dependencies are `serde` and `sha2` only (std threads,
no rayon or async runtime).

## Migration plan (which audit P1 each step fixes)

1. **Paddock** (this change): ADR, crate, `pd-job`, CI job. Nothing else changes.
2. **Debrief** (first consumer; its queue and `jobs` module stay): replace `ProgressSnapshot`/`JobProgress` phases with `Phase`; drive
   the existing Jobs pane with `pd-job`; fixes audit rows **1** (import tail phases + cancel between groups), **2** (phases and
   cancel for reload, build-beside-and-swap), **3** (save project as a job), **4** (redacted copy: bounded `par_map_ordered` over
   sources, forwarded inner progress), **5** (Chromium install: `copy_hashed`, phases Downloading/Unpacking/Testing), **6**
   (export progress), **7** (compaction/index phases), **8** (dumps as a job), plus the known AI download-then-checksum item
   (`copy_hashed`) and large-folder import (`par_walk` with live found-count feeding `pipeline_ordered`).
3. **Pitwall**: both Debrief-style bugs (engine setup, library download) become `copy_hashed` + phases; cancel through the token.
4. **Pitcrew**: `ServiceEvent::Progress` replaced by the shared snapshot; cleanup scan, disk map, project detection on `par_walk`;
   delete pipeline on `pipeline_ordered`; remove fixed UI timeouts (audit steps 1 to 6; its P1s: scan, issues, bench kill).
5. **Spotter**: check-all/scheduler tick, release download with hash-while-writing, Chromium install and folder scan as jobs
   (audit "foundation" rows).
6. **Dyno**: its scan/diagnostics and cleanup operations as jobs with the shared registry (audit rows #1 to #5, #7, #9).
7. Each app adds `assert_nonblocking` to its command tests and the grep check to CI, and deletes its own progress types and timeouts
   (no backward-compatibility shims before v1).

## Consequences

- One vocabulary and one component for progress in all apps; phases and the cancel guarantee are enforced by types and tests, not
  by convention. Engines still own the domain logic; the crate is mechanism only.
- Wire values are u64 and travel as JSON numbers: counts above 2^53 are not supported (not reachable for files or bytes in practice).
- `par_walk` buffers all entries for the sorted result; for trees too large for memory a streaming variant (sink instead of `Vec`) is
  the extension point (open question 1).
- Parallel speedups depend on storage: on a warm local cache the walk is ~1.3x faster than a sorted sequential walk (benchmark in
  `crates/paddock-jobs/benches/walk.rs`); gains are expected to be larger on cold caches and network shares and should be measured
  per app.

## Open questions

1. Streaming `par_walk` variant (callback per entry, unsorted) for very large trees; Debrief's import wants it first.
2. A shared `paddock-jobs-tauri` adapter crate once three apps carry the same 10-line sink and the `start/cancel/list` commands.
3. Persisting finished jobs (history) stays per app for now (Debrief has `jobs/persist.rs`).
