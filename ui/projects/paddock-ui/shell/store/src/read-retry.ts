import { DestroyRef, InjectionToken, type ProviderToken, type Signal } from '@angular/core';
import { patchState, type WritableStateSource } from '@ngrx/signals';
import type { RequestStatus } from './request-status';

/**
 * While a store's read is in error, it reads again on its own: after these delays in turn (a manual Retry starts over),
 * then every `READ_RETRY_STEADY_MS`. Bounded, so a failure that persists costs one read per 15 s at most (ADR 0058).
 */
export const READ_RETRY_BACKOFF_MS: readonly number[] = [2_000, 4_000, 8_000];

/** The delay between retries once `READ_RETRY_BACKOFF_MS` is used up. */
export const READ_RETRY_STEADY_MS = 15_000;

/**
 * The rate limit of event-started re-reads: after a re-read failed, the next event starts one only when this long has
 * passed since that failure (the timer's own retries are spaced by the backoff instead). Events during a re-read that is
 * still running start none (one read at a time).
 */
export const READ_RETRY_EVENT_SPACING_MS = 10_000;

/**
 * The clock of the self-retry: the time (for the event rate limit) and a one-shot timer. A port, so specs drive it by
 * hand (`FakeClock` of `@decryp7/paddock-ui/shell/testing`) and nothing waits for real time. `JobsClock` is one.
 */
export interface ReadRetryClock {
  now(): number;
  /** Calls `fire` once after `ms` unless the returned function is called first. */
  after(ms: number, fire: () => void): () => void;
}

export const READ_RETRY_CLOCK = new InjectionToken<ReadRetryClock>('PADDOCK_READ_RETRY_CLOCK', {
  providedIn: 'root',
  factory: () => ({
    now: () => Date.now(),
    after: (ms, fire) => {
      const handle = setTimeout(fire, ms);
      return () => clearTimeout(handle);
    },
  }),
});

/**
 * What a store declares about its request status (ADR 0058), as the argument of `withRequestStatus`. Every store
 * declares one; the spec `request-status-stores.spec.ts` checks each declaration against the store's behaviour.
 */
export type ReadPolicy =
  /**
   * The store reads state the engine owns. A failed read retries itself with backoff on `clock` (default
   * `READ_RETRY_CLOCK`), an event that arrives while in error ends the error (a whole snapshot) or starts a re-read (a
   * change), and the store runs its read through `store._reads.define(...)`.
   */
  | { readonly retry: true; readonly clock?: ProviderToken<ReadRetryClock> }
  /**
   * `permanent`: the status is not a read of engine state (a search, a validation, an action), or the store recovers in
   * its own way; `why` says which.
   */
  | { readonly retry: false; readonly kind: 'permanent'; readonly why: string }
  /**
   * `temporary`: the store reads engine state but has not adopted the retry yet; `until` (YYYY-MM-DD) is the date by which
   * it does. Conformance check C98 fails once that date has passed (the unit suite does not read the calendar).
   */
  | { readonly retry: false; readonly kind: 'temporary'; readonly why: string; readonly until: string };

/** A store's one read of engine state, declared once in its `withMethods`. */
export interface ReadSpec<T> {
  /** Asks the engine. Read store state before its first `await` (rule 29). */
  readonly read: () => Promise<T>;
  /**
   * Takes the answer, in a synchronous step right after it arrived and only when no newer read started since. It may
   * return more work that `load` waits for (it must not reject). The status becomes `'loaded'` right after it.
   */
  readonly apply: (value: T) => void | Promise<unknown>;
  /** The message of a failed read, as the user reads it. */
  readonly message: (e: unknown) => string;
  /** While true, `load` keeps the data on screen (no `'loading'`; an error stays up until the answer). Default: never. */
  readonly quiet?: () => boolean;
  /** The event rate limit after a failed re-read, ms. Default `READ_RETRY_EVENT_SPACING_MS`. */
  readonly eventSpacingMs?: number;
}

/** The handle a store calls: the read itself, and what its events say. */
export interface Reads {
  /**
   * Reads now (the first read and the user's Retry): stops the self-retry, shows `'loading'` unless `quiet`, and on a
   * failure shows the error and starts the backoff again from its first delay. `read` replaces the declared read for
   * this call only (diagnostics' "assemble again with this description"). Resolves true when the answer was applied.
   */
  load(read?: () => Promise<unknown>): Promise<boolean>;
  /**
   * An event arrived that changes part of the state (a job, an entry, a section). While in error the store reads the
   * whole state again: at most one read at a time, rate-limited after a failure (`eventSpacingMs`), keeping the error up
   * (no `'loading'`) until a read succeeds. Otherwise nothing.
   */
  event(): void;
  /**
   * The whole state arrived by another path (an event that carries a snapshot, a command's reply with the full view): an
   * error or a pending load ends as `'loaded'`, the self-retry stops, and a read still running is dropped (it is older).
   */
  received(): void;
}

/** The slice of a store the control needs. */
type StatusStore = WritableStateSource<{ requestStatus: RequestStatus }> & { readonly requestStatus: Signal<RequestStatus> };

/** What `withRequestStatus` adds as the private `_reads` prop. */
export interface ReadControl {
  /** Declares the store's read and returns the handle to call. */
  define<T>(spec: ReadSpec<T>): Reads;
}

/** The control behind `_reads`. `clock` is null for `retry: false`: then nothing retries and events start nothing. */
export function readControl(store: StatusStore, clock: ReadRetryClock | null, destroyRef: DestroyRef): ReadControl {
  return {
    define<T>(spec: ReadSpec<T>): Reads {
      const spacing = spec.eventSpacingMs ?? READ_RETRY_EVENT_SPACING_MS;
      /** Counts reads; a read that finishes after a newer one started (or after `received`) is dropped. */
      let generation = 0;
      /** The generation of the re-read in flight, or null: only a re-read of the current generation blocks another. */
      let refreshing: number | null = null;
      let lastRefreshFailedAt = Number.NEGATIVE_INFINITY;
      /** The pending self-retry, and how many were armed since the last success or `load`. */
      let cancelRetry: (() => void) | undefined;
      let retries = 0;
      let destroyed = false;

      const stopRetrying = (): void => {
        cancelRetry?.();
        cancelRetry = undefined;
        retries = 0;
      };
      destroyRef.onDestroy(() => {
        destroyed = true;
        stopRetrying();
      });

      const inError = (): boolean => typeof store.requestStatus() === 'object';

      /** Shows the error; the same message again changes nothing (the alert is not announced twice). */
      const fail = (e: unknown): void => {
        const error = spec.message(e);
        const now = store.requestStatus();
        if (typeof now === 'object' && now.error === error) return;
        patchState(store, { requestStatus: { error } });
      };

      /** Arms the next self-retry (replacing a pending one); the delays grow, see `READ_RETRY_BACKOFF_MS`. */
      const armRetry = (): void => {
        if (destroyed || !clock) return;
        cancelRetry?.();
        const delay = READ_RETRY_BACKOFF_MS[retries] ?? READ_RETRY_STEADY_MS;
        retries++;
        cancelRetry = clock.after(delay, () => {
          cancelRetry = undefined;
          void refresh(true);
        });
      };

      /** Applies an answer and marks the status loaded; returns the work `apply` handed back. */
      const take = (value: unknown): void | Promise<unknown> => {
        const more = spec.apply(value as T);
        patchState(store, { requestStatus: 'loaded' });
        return more;
      };

      /**
       * The re-read of a store in error, started by an event (`own` false) or by the self-retry timer (`own` true: no
       * event may ever come). The error stays up until a read succeeds; one read of the current generation at a time
       * (a stale one that still hangs blocks nothing); success stops the retrying. A timer that finds the current re-read
       * still running arms the next delay, so a read that never settles cannot end the retrying.
       */
      const refresh = async (own: boolean): Promise<void> => {
        if (destroyed || !clock) return;
        if (refreshing === generation) {
          if (own) armRetry();
          return;
        }
        if (!own && clock.now() - lastRefreshFailedAt < spacing) return;
        const mine = ++generation;
        refreshing = mine;
        try {
          const value = await spec.read();
          // Applied in a sync step of its own: the state is read fresh there, not across the await (rule 29).
          if (mine === generation) {
            const more = take(value);
            stopRetrying();
            await more;
          }
        } catch (e) {
          lastRefreshFailedAt = clock.now();
          if (mine === generation) {
            fail(e);
            armRetry();
          }
        } finally {
          if (refreshing === mine) refreshing = null;
        }
      };

      return {
        async load(read = spec.read): Promise<boolean> {
          const mine = ++generation;
          stopRetrying();
          if (!spec.quiet?.()) patchState(store, { requestStatus: 'loading' });
          try {
            const value = await read();
            if (mine !== generation) return false;
            await take(value);
            return true;
          } catch (e) {
            if (mine === generation) {
              fail(e);
              armRetry();
            }
            return false;
          }
        },
        event(): void {
          if (inError()) void refresh(false);
        },
        received(): void {
          if (destroyed) return;
          // Always newer than a read still in flight (a quiet load while loaded, too): that read is dropped.
          generation++;
          if (store.requestStatus() === 'loaded') return;
          stopRetrying();
          patchState(store, { requestStatus: 'loaded' });
        },
      };
    },
  };
}
