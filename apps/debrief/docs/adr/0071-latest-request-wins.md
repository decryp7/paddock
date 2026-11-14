# 71. The latest request wins: one helper for late async answers in the UI

- Status: **Accepted 2026-11-13** (user approved the redesign of the class "the UI acts on a late async answer"; the
  coordinator relayed it with the review of `fix/debrief-e2e-recovery-reload`).
- Date: 2026-11-13
- Related: Paddock rule 55 (one implementation per mechanism), ER-11, ER-21, ER-27, ER-37, ER-41; ADR 0058 (start-up
  contract, recovery reload).
- Evidence heads (ADR 0091): `origin/train/2026-11-13-9` `adf56b5e7`, the base of `origin/fix/debrief-e2e-recovery-reload`
  `1416fbfa8`. Nothing here is claimed as shipped.

## Context

The Debrief UI asks the engine many things whose answers can arrive after the question stopped mattering: the user
typed again, the query changed, a newer request was sent, or the component was destroyed. Each place guards this by hand
with a counter (`const mine = ++this.seq; ...; if (mine !== this.seq) return;`). There are about 35 copies
(`grep -E "(mine|seq|gen|generation) !==" src/app`, 2026-11-13). They differ in ways that caused real bugs:

- `EventsGrid.jumpTo` dropped a jump whose answer came back after a query change. A recovery reload then lost the
  selected event (`startup-contract.e2e.ts:120`, run 8056, jobs 37697 and 37699, 3 of 154 runs; fixed in `92375f669`).
- That fix's first version re-asked with no limit. Live tail bumps the grid's generation on every data change, so it
  could spin. A late error ended the jump without a word, and a user's click could be overwritten by a late answer
  (review findings on the same branch).
- No counter is cancelled when its component is destroyed, so a late answer can still write into a store.

## Decision

1. **One helper, `LatestRequest`** (`app/src/app/shared/latest-request.ts`). It does "latest request wins, cancel on
   destroy":
   - `run(request)` makes every earlier request stale and answers `done`, `failed` or `stale`.
   - `cancel()` makes the request in flight stale, for example when the user's own click must win.
   - A `DestroyRef` passed to the constructor cancels for good.
   - The request gets an `AbortSignal`, so multi-step work can stop early.
   - `begin()` and `LatestRequest.settle(signal, ...)` cover a round of several requests that belong together (the
     pages of one query).

   A stale answer or error is never acted on; that is the helper's contract, tested once (rule 55).
2. **A request that must survive a change re-asks deliberately, with a cap.** Today that is only the grid's jump. It
   asks again only when the query or sort order it asked about changed, at most `JUMP_ATTEMPTS = 3` times, then shows
   an error and logs it. A live refresh does not change the query and does not count.
3. **Migration.** The grid (jump, selected-event check) moves first, then the stores and cards that had hand-rolled
   counters: chart-card, smart-search, level-counts, query-session, context-section, the runs and annotations stores,
   config-diff, redacted-copy, format-wizard, tour and paged-rows.
4. **A lint, report-only (ER-41).** `no-restricted-syntax` flags a hand-rolled generation counter (`++seq`,
   `++this.generation` and the like) in `src/`. It warns until its first green CI run, then becomes an error. The
   remaining counters it lists are moved when their files are next touched.

## Common practice (ER-37) and cost

- RxJS `switchMap` (cancel the previous inner request when a new one starts) together with Angular's
  `takeUntilDestroyed` is the standard Angular answer
  ([rxjs.dev/api/operators/switchMap](https://rxjs.dev/api/operators/switchMap),
  [angular.dev/api/core/rxjs-interop/takeUntilDestroyed](https://angular.dev/api/core/rxjs-interop/takeUntilDestroyed)).
- In React, TanStack Query and SWR key each request and ignore results for keys no longer current.
- The web platform's own answer is `AbortController`
  ([MDN AbortController](https://developer.mozilla.org/en-US/docs/Web/API/AbortController)).
- `LatestRequest` is that behaviour for Debrief's `async`/`await` call sites. Rewriting them as Observables would touch
  every caller for the same result.
- Cost: about 1.5 agent-days. That is 0.5 for the helper, the grid and their tests, 0.75 for migrating the listed sites,
  and 0.25 for the lint and this ADR. The cheaper common-practice option is to keep hand-rolled counters and only fix
  the grid (0.25 day). It was declined: the class recurred (ER-21), and copies drift.

## Consequences

- One tested place decides what "stale" means. Call sites read `answer.status` instead of comparing counters.
- Destroyed components stop acting on answers.
- The remaining hand-rolled counters show up as lint warnings until they are moved.
