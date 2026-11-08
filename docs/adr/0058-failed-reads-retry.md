# 58. A failed read ends on its own: the declared read retry of `withRequestStatus`

- Status: Proposed (2026-11-08). The user approved the work on 2026-11-08 as the first shared item after the Dyno 1.1.0
  tag (Dyno 1.1.1 plan WP-1); the ADR waits for its independent review.
- Date: 2026-11-08
- Related: ER-21 / rule 40 (recurring bugs mean a missing design rule), rule 30 (one shared `withRequestStatus`), rule 29
  (signal reads before the first `await`), rule 34 (the engine is the single source of truth), rule 49 (no fixed turn
  waits in specs), rule 12 / ER-19 (the UI never times out work), ADR 0041 (deterministic timing in tests), rule 55 and
  ADR 0034 (one implementation per mechanism).

## Context

On 2026-11-08 Dyno's Jobs panel kept "The jobs could not be read" for good after one failed first read (the app started
while its agent restarted), even while it listed live jobs that later events brought. Retry cleared it. The fix for Dyno
1.1.0 (`JobsStore` on `origin/dyno/1.1.0-rc` 8efce3fc3) re-reads the list on a job event while in error, and, after the
user asked for it, also on its own on the injected `JOBS_CLOCK` (2 s, 4 s, 8 s, then every 15 s). Its reviews left two
gaps: no spec for "stop the self-retry when an event re-read succeeds" (a mutant that removed it survived) and an
identical error set again on every failed retry. The coordinator's search found the same shape in about eight more shell
stores (coordinator log 2026-11-08; plan `apps/dyno/docs/plans/dyno-1-1-1.md`, WP-1).

## Pattern

**A store that reads engine state keeps "could not be read" after one failed read, because nothing but the user's Retry
ever reads again or clears the status.** An event path that changes the data on screen never touches `requestStatus`,
and no store retries by itself, so an engine that comes back without sending an event leaves the error up forever.

The invariant: *a store in error after a failed read of engine state leaves the error on its own once the engine answers
again: through its own retry, through an event that carries the whole state, or through a re-read an event starts.*

## Instances (`withRequestStatus` call sites on `origin/main` 9e18230c5)

All 20 call sites in `@decryp7/paddock-ui`. "Never clears" means even an event leaves the error up; "event only" means
an event clears it, but without one (nothing changed in the engine) the error stays.

| Store (file under `ui/projects/paddock-ui/`) | Event | Before | After (this ADR) |
| --- | --- | --- | --- |
| `shell/jobs/src/jobs.store.ts` | one job (part) | fixed in 1.1.0 by its own copy of the logic | on the shared mechanism, `clock: JOBS_CLOCK` |
| `shell/undo/src/activity.store.ts` | one entry (part) | never clears | retry; an event re-reads |
| `shell/settings/src/settings.store.ts` | one section (part) | never clears | retry; an event re-reads |
| `shell/notify/src/service-health.store.ts` | whole snapshot | never clears | retry; an event or a repair reply clears |
| `shell/notify/src/notifications.store.ts` | whole list | never clears | retry; an event clears |
| `shell/channels/src/channels.store.ts` | a test's progress (part) | never clears | retry; an event re-reads; a save reply or quiet refresh clears |
| `shell/mcp/src/mcp.store.ts` | "the log changed" (part) | never clears | retry; an event re-reads; a save, revoke or refresh reply clears |
| `shell/diagnostics/src/diagnostics.store.ts` | a save outcome (part) | never clears | retry; an event collects again |
| `shell/about/src/updates.store.ts` | whole status | never clears | retry; an event or a step's reply clears |
| `shell/setup/src/setup.store.ts` | whole state | event only | retry; an event clears |
| `shell/storage/src/storage.store.ts` | whole usage (`usage` event) | event only | retry; an event clears |
| `shell/packs/src/packs.store.ts` | whole view | event only | retry; an event or a reload reply clears |
| `shell/pitwall/src/pitwall.store.ts` | whole snapshot | event only | retry; an event or an action's snapshot clears |
| `shell/files/src/files.store.ts` | whole lists | event only | retry; an event clears |
| `shell/service/src/service.store.ts` | whole status | event only | retry; an event clears |
| `shell/startup/src/recovery.store.ts` | none | fine: asked once, a failure shows nothing | `retry: false` with that reason |
| `workbench/palette/src/palette.store.ts` | none | fine: one search per query | `retry: false` |
| `view/src/view.store.ts` | none | fine: the status is a validation | `retry: false` |
| `workbench/assistant/src/assistant.store.ts` | conversation events | at risk (load error stays until Retry) | `retry: false`, "not adopted yet" (rollout step 3) |
| `live/src/live-feed.store.ts` | frames | at risk (a failed resync stays until a gap or Retry) | `retry: false`, "not adopted yet" (rollout step 3) |

So the list in the plan was right about the eight that never clear, and short by six: setup, storage, packs, pitwall,
files and service clear on an event but never retry, so the same bug shows whenever the engine comes back unchanged.

Debrief has its own `withRequestStatus` (`apps/debrief/app/src/app/shared/store/with-request-status.ts`, 11 stores); it
is not a user of the shared feature and is outside this ADR (see Rollout, step 4).

## Decision

### 1. Every store declares its read policy

`withRequestStatus(policy: ReadPolicy)` takes a required argument (`ui/projects/paddock-ui/shell/store/src/request-status.ts`,
`read-retry.ts`):

- `{ retry: true, clock? }`: the store reads engine state. The feature's private prop `_reads` runs its read.
- `{ retry: false, why }`: the status is not a read of engine state (a search, a validation, an action's progress), or
  the store recovers in its own way; `why` says which.

A store that forgets the argument does not compile.

### 2. One mechanism runs the read

In its `withMethods` a `retry: true` store declares its one read once:

```ts
const reads = store._reads.define({
  read: () => backend.overview(),            // reads store state before its first await (rule 29)
  apply: (overview) => patchState(store, { overview }),
  message: (e) => errorMessage(e, $localize`...`),
  quiet: () => store.overview() !== null,     // optional: keep data on screen, no 'loading' flash
});
```

and then calls three things, never `setLoading`/`setLoaded`/`setError` for that read:

- `reads.load(read?)`: the first read and the user's Retry. It stops the self-retry, shows `'loading'` unless `quiet`, and
  on a failure shows the error and starts the backoff from its first delay. A one-off `read` replaces the declared one for
  this call (diagnostics' "assemble again with this description"). It resolves true when the answer was applied.
- `reads.event()`: an event that carries part of the state arrived. While in error the store reads the whole state
  again; otherwise nothing.
- `reads.received()`: the whole state arrived by another path (an event with a snapshot, a command's reply with the full
  view). An error or a pending load ends as `'loaded'`, the self-retry stops, and a read still running is dropped.

The behaviour, moved unchanged from the 1.1.0 Jobs store and now shared:

- **Self-retry with backoff** on the injected clock (`READ_RETRY_CLOCK`, or the store's own, such as `JOBS_CLOCK`): after
  2 s, 4 s and 8 s, then every 15 s (`READ_RETRY_BACKOFF_MS`, `READ_RETRY_STEADY_MS`). Bounded: a failure that persists
  costs one read per 15 s. Nothing is timed out (rule 12).
- **No loading flash:** a retry or event re-read keeps the error up (no `'loading'`), so the alert is not removed and
  announced again; the same message again is not set again (the status object stays, so assistive technology does not
  announce it twice).
- **One read at a time:** a timer or event that comes during a re-read starts none.
- **Rate limit for events:** after a failed re-read, an event starts the next one only once `eventSpacingMs` has passed
  since that failure (default `READ_RETRY_EVENT_SPACING_MS`, 10 s; the Jobs store keeps its `stillWorkingAfterMs`). The
  timer's own retries are spaced by the backoff instead.
- **Generation counter:** a read that finishes after a newer one started (a Retry, a `received`) is dropped.
- **Success stops everything:** the timer is cancelled and the backoff resets, whether the success came from the timer,
  an event re-read, a `load` or `received`.
- **Destroy guard:** destroying the store cancels the timer, and a read that fails afterwards arms none.
- **Failures that are not the read** (an action refused, notifications' mark-as-read) still use `setError`; they arm no
  retry. A later `received` clears them, which is right: the whole state is newer than the failure.

### 3. The check over every instance

- **Compile time:** the policy argument is required, so every call site declares one.
- **`shell/testing/src/request-status-stores.spec.ts`** scans every non-spec source under `ui/projects/paddock-ui` for
  `withRequestStatus(` and requires a row for each file, with the policy the file declares; a store added later without a
  row fails. Every `retry: false` must give a reason. For every `retry: true` row it builds the store over its memory
  backend with a first read that fails, then checks: the self-retry reads again after 2 s and the error ends; an event
  that carries the whole state ends the error at once without a read; an event that carries a part starts a re-read whose
  success ends the error; destroying the store leaves no timer. On today's main (with the stores declared `retry: false`
  for the run) it fails 23 cases: the self-retry of all 14 stores but Jobs, and the event case of the eight that never
  clear. After the adoption all 47 cases pass.
- **`shell/store/src/read-retry.spec.ts`** holds the mechanism to the behaviour above, with the Jobs store's two gaps:
  stop on success after an event re-read, and no re-set of an identical error. Seven mutants of `read-retry.ts` (no stop on
  a re-read's success, the identical error set again, `received` keeping the old read, a timer armed after destroy, no
  event rate limit, events ignored, a Retry that keeps the old backoff) each fail at least one case.
- The Jobs store's own specs stay as they were and pass on the shared mechanism, with the same two gaps added and the
  two-microtask wait replaced by one macrotask (rule 49).

### 4. Rule 30 (proposed amendment)

"Request state is a status union ... from one shared `withRequestStatus` feature, never separate `loading`/`error` flags,
**and every store declares its `ReadPolicy`: a store that reads engine state runs that read through `_reads`, so a failed
read ends on its own (ADR 0058).**" The check is the spec above, so the rule is not a reminder.

## Rollout

1. The mechanism in `@decryp7/paddock-ui/shell/store`, and the Jobs store moved onto it (its `JOBS_RETRY_*` constants go;
   they were never exported). Branch `feat/shared-read-retry`.
2. The 14 other shell stores of the table adopt it in the same branch (rule 39: a shared change and its adoption land
   together).
3. The workbench assistant and the live feed adopt it in a follow-up; until then they declare `retry: false` with
   "not adopted yet", which the spec allows but which names the step.
4. Debrief's own `withRequestStatus` (11 stores, its own note says to move it to `paddock-ui` when a second app needs it)
   moves onto the shared feature with Debrief 1.2, or is registered in `docs/known-copies.toml` with a delete-by tag; the
   coordinator decides which (rule 55).

Rollback: revert the branch. The mechanism has no persisted state and no wire change.

## Same pass: the Rust backoffs (rule 55)

The two places the plan named are not copies of each other:

- `crates/paddock-ipc/src/link/client.rs` already uses the family's one stateful backoff, `paddock_jobs::time::Backoff`
  with `BackoffPolicy::retry_forever(250 ms, 15 s, 60 s)`. Nothing to share.
- `crates/paddock-jobs/src/schedule/mod.rs` `next_delay_secs` is a different thing: the interval of a schedule doubled
  per failure with ±10 % jitter and a 6 h cap, stateless over a failure count. It lives in the same crate as `Backoff`.

The real copies of the "doubling delay capped at a maximum" formula, outside `Backoff`, are:
`apps/debrief/crates/debrief-ipc/src/supervisor.rs` (`backoff_delay`), `apps/debrief/crates/debrief-ai/src/updates/policy.rs`
(`backoff_secs`), `apps/dyno/crates/dyno-agent/src/helper_first.rs` (`backoff`), `apps/dyno/crates/dyno-plugin-host/src/supervision.rs`
(`backoff`), `apps/dyno/app/src-tauri/src/link/mod.rs` (`backoff`) and `crates/paddock-download/src/config.rs`
(`backoff_for`). (`apps/spotter/crates/spotter-core/src/schedule.rs` `next_delay_secs` is the same code as the schedule's;
it is already registered under the `schedules` mechanism, delete by `spotter-v1.1.0`.) Proposal, not built here: a pure
`BackoffPolicy::delay_after(failures: u32) -> Duration` in `paddock_jobs::time` (the formula `Backoff` uses), the six
call sites moved onto it, and until then one `exponential-backoff` mechanism in `docs/known-copies.toml` with
`paddock-jobs` as canonical and each copy's owner and delete-by tag (the coordinator sets the tags; not added here, since
an entry without agreed tags would block those apps' tags under C58).

## Alternatives rejected

- **Fix each store by hand** (what 1.1.0 did for Jobs): eight more copies of a guard, a generation counter and a timer,
  and nothing stops the ninth store. ER-21 says no.
- **Retry in the backends** (every Tauri backend retries its commands): a backend cannot know which calls are reads that
  may repeat, it would hide errors the user must see, and it does not clear a status a store already set.
- **`resource()` with a reload timer** (rule 36): fits new read-only loading, but these stores mix reads with commands,
  events and replies that carry the whole state; a resource's status would sit next to `requestStatus` instead of
  replacing it, and the migration is larger than the bug.
- **Make every event carry the whole state:** right for some (it is what `received` serves), wrong for a job list or an
  activity log, and it still leaves an engine that sends no event.

## Consequences

- A read of engine state has one implementation; a new store gets the retry by declaring `retry: true` and the spec
  holds it to it.
- A persisting failure costs one read per 15 s per open store; the stores live in windows that are open.
- `withRequestStatus()` without an argument is a breaking change of `@decryp7/paddock-ui/shell/store` (allowed before v1;
  CHANGELOG entry). Debrief is not affected (own copy).
