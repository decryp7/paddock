import { inject } from '@angular/core';
import { patchState, signalStore, withMethods, withState } from '@ngrx/signals';
import type { CommandError, LifespanOverview } from '../core/api.types';
import { BACKEND } from '../core/backend';
import { toCommandError } from '../core/command-error';

interface LifespanState {
  overview: LifespanOverview | null;
  error: CommandError | null;
}

/** How long disks and batteries have left (WP-24). Forecasts move slowly, so the page reads them once per visit. A query. */
export const LifespanStore = signalStore(
  { providedIn: 'root' },
  withState<LifespanState>({ overview: null, error: null }),
  withMethods((store, backend = inject(BACKEND)) => ({
    async load(): Promise<void> {
      try {
        patchState(store, { overview: await backend.lifespan(), error: null });
      } catch (e) {
        patchState(store, { error: toCommandError(e) });
      }
    },
  })),
);
