import { DestroyRef, inject } from '@angular/core';
import { patchState, signalStore, withMethods, withState } from '@ngrx/signals';
import type { CommandError, ThrottleOverview } from '../core/api.types';
import { BACKEND } from '../core/backend';
import { toCommandError } from '../core/command-error';

/** The Throttling page asks again this often while it is open. */
export const THROTTLE_REFRESH_MS = 15_000;

interface ThrottleState {
  overview: ThrottleOverview | null;
  error: CommandError | null;
}

/** Whether the CPU is slowed down and why (WP-23): the latest answer, refreshed only while the page is open. A query: it changes nothing. */
export const ThrottleStore = signalStore(
  { providedIn: 'root' },
  withState<ThrottleState>({ overview: null, error: null }),
  withMethods((store, backend = inject(BACKEND), destroyRef = inject(DestroyRef, { optional: true })) => {
    let timer: ReturnType<typeof setInterval> | null = null;
    const stop = () => {
      if (timer !== null) clearInterval(timer);
      timer = null;
    };
    const load = async (): Promise<void> => {
      try {
        patchState(store, { overview: await backend.throttling(), error: null });
      } catch (e) {
        patchState(store, { error: toCommandError(e) });
      }
    };
    destroyRef?.onDestroy(stop);
    return {
      load,
      /** Loads now and then every {@link THROTTLE_REFRESH_MS} until `stop`. */
      async start(): Promise<void> {
        stop();
        timer = setInterval(() => void load(), THROTTLE_REFRESH_MS);
        await load();
      },
      stop,
    };
  }),
);
