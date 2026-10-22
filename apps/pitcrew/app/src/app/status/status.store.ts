import { inject } from '@angular/core';
import { patchState, signalStore, withMethods, withState } from '@ngrx/signals';
import type { CommandError, RunnerSnapshot } from '../core/api.types';
import { BACKEND } from '../core/backend';
import { toCommandError } from '../core/command-error';

interface StatusState {
  runners: RunnerSnapshot[];
  loading: boolean;
  loaded: boolean;
  error: CommandError | null;
}

const initial: StatusState = { runners: [], loading: false, loaded: false, error: null };

/** The status view's state: the runners as the service reports them, and the last request's error. */
export const StatusStore = signalStore(
  withState(initial),
  withMethods((store, backend = inject(BACKEND)) => ({
    /** Fetches every runner's status; a failure keeps the last good list and shows the error with Retry. */
    async load(): Promise<void> {
      patchState(store, { loading: true, error: null });
      try {
        const runners = await backend.status();
        patchState(store, { runners, loading: false, loaded: true });
      } catch (e) {
        patchState(store, { loading: false, error: toCommandError(e) });
      }
    },
  })),
);
