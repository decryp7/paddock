import { computed } from '@angular/core';
import { signalStoreFeature, withComputed, withState } from '@ngrx/signals';

/**
 * Where the store's last request is (rule 30): one union, never separate `loading`/`error` flags. The jobs entry point's
 * copy of the shape `@decryp7/paddock-ui/shell/store` publishes as `withRequestStatus` (plan S4); it moves there then.
 */
export type RequestStatus = 'idle' | 'loading' | 'loaded' | { readonly error: string };

/** `requestStatus` plus `isLoading`, `isLoaded` and `requestError` (the message, or null). */
export function withRequestStatus() {
  return signalStoreFeature(
    withState<{ requestStatus: RequestStatus }>({ requestStatus: 'idle' }),
    withComputed(({ requestStatus }) => ({
      isLoading: computed(() => requestStatus() === 'loading'),
      isLoaded: computed(() => requestStatus() === 'loaded'),
      requestError: computed(() => {
        const s = requestStatus();
        return typeof s === 'object' ? s.error : null;
      }),
    })),
  );
}

export const loading = (): { requestStatus: RequestStatus } => ({ requestStatus: 'loading' });
export const loaded = (): { requestStatus: RequestStatus } => ({ requestStatus: 'loaded' });
export const failed = (error: string): { requestStatus: RequestStatus } => ({ requestStatus: { error } });
