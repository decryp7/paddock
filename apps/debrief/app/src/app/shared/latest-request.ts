import { DestroyRef } from '@angular/core';

/**
 * What became of a request started with {@link LatestRequest}: its answer (`done`), its error (`failed`, only while it
 * was still the latest), or `stale` when a newer request, `cancel()` or the owner's destruction made it obsolete. A
 * stale answer and a stale error are never acted on (ADR 0071).
 */
export type Answer<T> = { readonly status: 'done'; readonly value: T } | { readonly status: 'failed'; readonly error: unknown } | { readonly status: 'stale' };

const STALE: Answer<never> = { status: 'stale' };

/**
 * One request in flight at a time: "the latest request wins, cancel on destroy" (ADR 0071). Starting a request makes
 * every earlier one stale; `cancel()` does the same without starting one (a user's click wins over a late answer); the
 * owner's `DestroyRef` cancels for good. The request gets an `AbortSignal` that is aborted when it goes stale, so a
 * cancellable backend call or a multi-step request can stop early (`signal.aborted`).
 *
 * The same behaviour as RxJS `switchMap` with `takeUntilDestroyed`, for the `async`/`await` call sites Debrief has.
 * Use `begin()` when several requests belong to one round (the pages of one query): they share one signal, and
 * `begin()` again ends the round.
 */
export class LatestRequest {
  private controller: AbortController | null = null;
  private destroyed = false;

  constructor(destroyRef?: DestroyRef) {
    destroyRef?.onDestroy(() => {
      this.destroyed = true;
      this.cancel();
    });
  }

  /** Whether a request (or round) is in flight and still the latest. */
  get busy(): boolean {
    return this.controller !== null && !this.controller.signal.aborted;
  }

  /** Starts a new round: earlier requests go stale. A destroyed owner gets an already aborted signal. */
  begin(): AbortSignal {
    this.controller?.abort();
    const controller = new AbortController();
    if (this.destroyed) controller.abort();
    this.controller = controller;
    return controller.signal;
  }

  /** Makes the request in flight stale without starting another one. */
  cancel(): void {
    this.controller?.abort();
    this.controller = null;
  }

  /** Starts `request` as the latest one and answers how it ended. */
  run<T>(request: (signal: AbortSignal) => Promise<T>): Promise<Answer<T>> {
    return LatestRequest.settle(this.begin(), request);
  }

  /** Runs `request` inside an existing round (`begin()`): stale once that round ended. */
  static async settle<T>(signal: AbortSignal, request: (signal: AbortSignal) => Promise<T>): Promise<Answer<T>> {
    if (signal.aborted) return STALE;
    try {
      const value = await request(signal);
      return signal.aborted ? STALE : { status: 'done', value };
    } catch (error) {
      return signal.aborted ? STALE : { status: 'failed', error };
    }
  }
}
