import { inject } from '@angular/core';
import { patchState, signalStore, withMethods, withState } from '@ngrx/signals';
import type { CommandError, SystemInventory } from '../core/api.types';
import { BACKEND } from '../core/backend';
import { toCommandError } from '../core/command-error';

interface InventoryState {
  inventory: SystemInventory | null;
  loading: boolean;
  error: CommandError | null;
}

/** What is installed in this computer (memory modules, CPU, disks) and the engine's upgrade hints. Read once per visit; no side effects. */
export const InventoryStore = signalStore(
  { providedIn: 'root' },
  withState<InventoryState>({ inventory: null, loading: false, error: null }),
  withMethods((store, backend = inject(BACKEND)) => ({
    async load(): Promise<void> {
      patchState(store, { loading: true, error: null });
      try {
        patchState(store, { inventory: await backend.inventory(), loading: false });
      } catch (e) {
        patchState(store, { loading: false, error: toCommandError(e) });
      }
    },
  })),
);
