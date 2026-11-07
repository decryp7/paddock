import { inject } from '@angular/core';
import { patchState, signalStore, withHooks, withMethods, withState } from '@ngrx/signals';
import type { CommandError, WakeInput, WakesView } from '../core/api.types';
import { BACKEND, type Unsubscribe } from '../core/backend';
import { toCommandError } from '../core/command-error';

interface WakesState {
  view: WakesView | null;
  error: CommandError | null;
  /** A save or cancel is on its way to the agent. */
  busy: boolean;
  /** Why the last save or cancel failed (the list stays as it was). */
  actionError: CommandError | null;
}

/** Scheduled wake: the agent's schedules and who sets the OS wake. The agent validates and decides; this store only asks. */
export const WakesStore = signalStore(
  { providedIn: 'root' },
  withState<WakesState>({ view: null, error: null, busy: false, actionError: null }),
  withMethods((store, backend = inject(BACKEND)) => {
    async function act(call: () => Promise<WakesView>): Promise<boolean> {
      patchState(store, { busy: true, actionError: null });
      try {
        patchState(store, { view: await call(), error: null });
        return true;
      } catch (e) {
        patchState(store, { actionError: toCommandError(e) });
        return false;
      } finally {
        patchState(store, { busy: false });
      }
    }
    return {
      async load(): Promise<void> {
        try {
          patchState(store, { view: await backend.powerWakes(), error: null });
        } catch (e) {
          patchState(store, { error: toCommandError(e) });
        }
      },
      save: (entry: WakeInput) => act(() => backend.powerSetWake(entry)),
      cancel: (id: number) => act(() => backend.powerCancelWake(id)),
      clearActionError(): void {
        patchState(store, { actionError: null });
      },
      receive(view: WakesView): void {
        patchState(store, { view, error: null });
      },
    };
  }),
  withHooks((store) => {
    const backend = inject(BACKEND);
    let stop: Unsubscribe | null = null;
    return {
      onInit() {
        stop = backend.onWakes((v) => store.receive(v));
      },
      onDestroy() {
        stop?.();
      },
    };
  }),
);
