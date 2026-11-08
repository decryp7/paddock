import { computed, DestroyRef, inject } from '@angular/core';
import { signalStoreFeature, withComputed, withProps, withState } from '@ngrx/signals';
import { READ_RETRY_CLOCK, readControl, type ReadPolicy } from './read-retry';

/**
 * Where a store's last request is (rule 30): one union, never separate `loading`/`error` flags that can disagree. An
 * error carries the message for the user, as is.
 */
export type RequestStatus = 'idle' | 'loading' | 'loaded' | { readonly error: string };

/** The state slice the feature adds. */
export interface RequestStatusState {
  requestStatus: RequestStatus;
}

/**
 * The shared SignalStore feature of every shell and app store that loads something (rule 30): `requestStatus` (starts
 * `'idle'`) plus `isLoading`, `isLoaded` and `requestError` (the message, or null). Change it only from the store's own
 * methods, with `patchState(store, setLoading())`, `setLoaded()` or `setError(message)` (rule 31).
 *
 * `policy` is the store's declaration of what happens after a failed read (ADR 0058). `{ retry: true }`: the store runs
 * its read through the private `_reads` prop (`store._reads.define({ read, apply, message })` in its `withMethods`), which
 * retries a failed read on its own with backoff and ends an error when an event shows the engine answers again; then
 * `requestStatus` is changed only through that handle and `setError` is for failures that are not the read. `{ retry:
 * false, why }`: nothing retries; `why` says why the status is not a read of engine state. `request-status-stores.spec.ts`
 * checks every declaration against the store's behaviour.
 */
export function withRequestStatus(policy: ReadPolicy) {
  return signalStoreFeature(
    withState<RequestStatusState>({ requestStatus: 'idle' }),
    withComputed(({ requestStatus }) => ({
      isLoading: computed(() => requestStatus() === 'loading'),
      isLoaded: computed(() => requestStatus() === 'loaded'),
      requestError: computed(() => {
        const s = requestStatus();
        return typeof s === 'object' ? s.error : null;
      }),
    })),
    withProps((store) => ({
      _reads: readControl(store, policy.retry ? inject(policy.clock ?? READ_RETRY_CLOCK) : null, inject(DestroyRef)),
    })),
  );
}

/** The request started. */
export const setLoading = (): RequestStatusState => ({ requestStatus: 'loading' });
/** The request answered. */
export const setLoaded = (): RequestStatusState => ({ requestStatus: 'loaded' });
/** The request failed with `error` (shown to the user as is). */
export const setError = (error: string): RequestStatusState => ({ requestStatus: { error } });

/** The message of anything thrown or rejected: an `Error`'s message, a string, an object's `message`, else `fallback`. */
export function errorMessage(e: unknown, fallback: string): string {
  if (typeof e === 'string' && e) return e;
  if (e instanceof Error) return e.message;
  if (e && typeof e === 'object' && typeof (e as { message?: unknown }).message === 'string') return (e as { message: string }).message;
  return fallback;
}
