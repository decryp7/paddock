import { inject } from '@angular/core';
import { patchState, signalStore, withMethods, withState } from '@ngrx/signals';
import type { CommandError, EnergyReport } from '../core/api.types';
import { BACKEND } from '../core/backend';
import { toCommandError } from '../core/command-error';

interface EnergyState {
  report: EnergyReport | null;
  days: number;
  error: CommandError | null;
}

/** The energy report for the chosen range. The engine adds the numbers; this store only asks. */
export const EnergyStore = signalStore(
  { providedIn: 'root' },
  withState<EnergyState>({ report: null, days: 7, error: null }),
  withMethods((store, backend = inject(BACKEND)) => ({
    async load(days = store.days()): Promise<void> {
      patchState(store, { days });
      try {
        patchState(store, { report: await backend.energyReport(days), error: null });
      } catch (e) {
        patchState(store, { error: toCommandError(e) });
      }
    },
  })),
);
