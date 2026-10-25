# 5. Queue and scheduling

- Status: accepted (2026-10-22)
- Related: ADR 0003, ADR 0006, ADR 0007, ADR 0009; code: [`pitwall_core::scheduler`](../../crates/pitwall-core/src/scheduler.rs),
  [`pitwall-server/src/serving.rs`](../../crates/pitwall-server/src/serving.rs) (`acquire`, `caps_of`)

## Context

One machine, a few GPU or CPU slots, many apps. Debrief's chat must not wait behind another app's bulk indexing, but
bulk work must not starve either. Loading a model is slow and memory-bound, so admission must also respect per-model
and per-app limits. The rules must be testable without a clock.

## Decision

Every chat, completion and embedding request is a **job** in `Scheduler` (pure; time is an argument). The server owns the
waiting: `Serving::acquire` submits a job, then loops (woken by a `Notify`, with a 500 ms safety tick) calling `dispatch`
until the job runs, and returns a `Permit` that frees the slot when dropped.

- **Priorities**: `interactive` (default; someone is waiting), `background`, `batch`. An app asks with
  `X-Pitwall-Priority`; its permissions cap how urgent it may be (`AppPermissions.max_priority`, applied with
  `Priority::capped`, so a batch-only app asking for interactive runs as batch). An unknown value is a 400.
- **Dispatch order**: among waiting jobs that pass the caps, the lowest *effective* priority value (most urgent) wins, then
  earliest submission, then lowest id.
- **Aging**: every `aging` (30 s) a job has waited raises it one level, so batch work reaches background and then
  interactive and never starves.
- **Caps** (`Caps`): `max_running` jobs at once over all models (default 4), `per_model` at once on one model (default 2,
  matching llama-server's parallel slots), `max_waiting` queued jobs (default 256; more are refused), and an optional
  per-app running cap (`limits.maxConcurrent`). The server derives `max_running` from `server.maxConcurrent` (default 4,
  at least 1) and `per_model` as the same value capped at 4.
- **Refusals**: a full queue or a paused server (`set_serving`) is `overloaded` (503, `Retry-After: 5`). A request that waits
  too long hits the request timeout (default 600 s).
- **Cancel**: closing the connection stops a running job; `DELETE /x/v1/jobs/{id}` cancels a queued or running job (an app
  may cancel only its own; the admin any). `GET /x/v1/jobs` and the live feed show queue depth, waiting and running jobs,
  with their state, priority, model and tokens so far.
- **Order of work for one request**: authorize (model, tools, extended APIs), rate and token limits (ADR 0007), `acquire`,
  then `ensure` the model is loaded (ADR 0006: plan, evict idle models, load), then generate. Loads of one model are
  serialised by a per-model gate so a burst of cold requests loads it once.
- Job kinds (`chat`, `completion`, `embedding`, `transcription`, `task`, `batch`, `benchmark`) exist in the type; the server
  submits chat and embedding jobs today.

## Consequences

- Interactive work keeps its latency under load while batch work still finishes.
- Capability selection (`auto:...`) happens before queueing, so caps apply to the chosen model.
- Fairness between apps of the same priority is by arrival order plus the per-app cap; there is no weighted sharing.

## Not built yet

- Batch jobs with resume and progress events (`JobKind::Batch` and progress fields exist; no endpoint creates them).
- Benchmark jobs and benchmark-fed selection: `run_benchmark` is a stub and `benchmarks` returns an empty list, so
  `fast` selection falls back to the file-size heuristic.
- Task endpoints (`JobKind::Task`) and transcription jobs.
- Preemption: a running job is never paused for a more urgent one.
