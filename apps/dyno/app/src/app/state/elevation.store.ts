import { computed, inject } from '@angular/core';
import { patchState, signalStore, withComputed, withMethods, withState } from '@ngrx/signals';
import { BACKEND } from '../core/backend';
import { toCommandError } from '../core/command-error';
import { Notifier } from '../shared/notifier';
import { LiveStore } from './live.store';

interface ElevationState {
  /** A request is waiting for the OS prompt or the restart. */
  busy: boolean;
}

/**
 * "Run as administrator" for this session. The agent decides whether it runs elevated (the live update says so); this store
 * asks for the switch, shows a calm message when the prompt is cancelled and an error when it fails.
 */
export const ElevationStore = signalStore(
  { providedIn: 'root' },
  withState<ElevationState>({ busy: false }),
  withComputed((s, live = inject(LiveStore)) => ({
    elevated: computed(() => live.update()?.elevated ?? false),
    busy: computed(() => s.busy()),
  })),
  withMethods((store, backend = inject(BACKEND), notifier = inject(Notifier)) => {
    const run = async (call: () => ReturnType<typeof backend.runAsAdministrator>): Promise<void> => {
      if (store.busy()) return;
      patchState(store, { busy: true });
      try {
        const status = await call();
        if (status.message) notifier.show(status.message, 'ok');
      } catch (e) {
        notifier.error(toCommandError(e));
      } finally {
        patchState(store, { busy: false });
      }
    };
    return {
      /** Restarts the agent with administrator rights after the OS prompt. */
      runAsAdministrator: () => run(() => backend.runAsAdministrator()),
      /** Restarts the agent with normal rights. */
      goBackToNormal: () => run(() => backend.runUnelevated()),
    };
  }),
);
