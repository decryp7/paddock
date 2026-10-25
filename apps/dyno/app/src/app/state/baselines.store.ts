import { DestroyRef, inject } from '@angular/core';
import { patchState, signalStore, withMethods, withState } from '@ngrx/signals';
import type { BaselineOverview, CommandError } from '../core/api.types';
import { BACKEND } from '../core/backend';
import { toCommandError } from '../core/command-error';

/** The dashboard card asks again this often while the dashboard is open. */
export const BASELINES_REFRESH_MS = 60_000;

interface BaselineState {
  overview: BaselineOverview | null;
  error: CommandError | null;
}

/** "Is this normal?" (WP-25): the engine's judgement of now against what is usual for this time of the week, refreshed only while shown. */
export const BaselinesStore = signalStore(
  { providedIn: 'root' },
  withState<BaselineState>({ overview: null, error: null }),
  withMethods((store, backend = inject(BACKEND), destroyRef = inject(DestroyRef, { optional: true })) => {
    let timer: ReturnType<typeof setInterval> | null = null;
    const stop = () => {
      if (timer !== null) clearInterval(timer);
      timer = null;
    };
    const load = async (): Promise<void> => {
      try {
        patchState(store, { overview: await backend.baselines(), error: null });
      } catch (e) {
        patchState(store, { error: toCommandError(e) });
      }
    };
    destroyRef?.onDestroy(stop);
    return {
      load,
      /** Loads now and then every {@link BASELINES_REFRESH_MS} until `stop`. */
      async start(): Promise<void> {
        stop();
        timer = setInterval(() => void load(), BASELINES_REFRESH_MS);
        await load();
      },
      stop,
    };
  }),
);
