import { inject } from '@angular/core';
import { patchState, signalStore, withMethods, withState } from '@ngrx/signals';
import type { CommandError, FirewallView } from '../core/api.types';
import { BACKEND } from '../core/backend';
import { toCommandError } from '../core/command-error';

interface FirewallState {
  view: FirewallView | null;
  error: CommandError | null;
  loadedAt: number | null;
}

/** The firewall of this computer, read when the page opens and on Refresh (read-only). */
export const FirewallStore = signalStore(
  { providedIn: 'root' },
  withState<FirewallState>({ view: null, error: null, loadedAt: null }),
  withMethods((store, backend = inject(BACKEND)) => ({
    async load(): Promise<void> {
      try {
        patchState(store, { view: await backend.firewall(), error: null, loadedAt: Date.now() });
      } catch (e) {
        patchState(store, { error: toCommandError(e) });
      }
    },
  })),
);
