import { inject } from '@angular/core';
import { patchState, signalStore, withMethods, withState } from '@ngrx/signals';
import type { CommandError, UnusualOverview } from '../core/api.types';
import { BACKEND } from '../core/backend';
import { toCommandError } from '../core/command-error';
import { Notifier } from '../shared/notifier';

interface UnusualState {
  overview: UnusualOverview | null;
  error: CommandError | null;
}

/** New and unusual programs: the list the engine builds from its baseline. Dyno only lists them; it never acts on a program. */
export const UnusualStore = signalStore(
  { providedIn: 'root' },
  withState<UnusualState>({ overview: null, error: null }),
  withMethods((store, backend = inject(BACKEND), notifier = inject(Notifier)) => ({
    async load(): Promise<void> {
      try {
        patchState(store, { overview: await backend.unusual(), error: null });
      } catch (e) {
        patchState(store, { error: toCommandError(e) });
      }
    },
    /** Forgets the baseline; the caller has asked the person first. */
    async forget(): Promise<void> {
      try {
        patchState(store, { overview: await backend.forgetBaseline(), error: null });
        notifier.show('Dyno forgot the programs it knew and starts learning again');
      } catch (e) {
        notifier.error(toCommandError(e));
      }
    },
  })),
);
