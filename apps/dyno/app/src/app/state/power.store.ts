import { inject } from '@angular/core';
import { patchState, signalStore, withHooks, withMethods, withState } from '@ngrx/signals';
import type { AwakeSpec, CommandError, PowerView } from '../core/api.types';
import { BACKEND, type Unsubscribe } from '../core/backend';
import { toCommandError } from '../core/command-error';

interface PowerState {
  view: PowerView | null;
  error: CommandError | null;
  /** A start or stop is on its way to the agent. */
  busy: boolean;
  /** Why the last start or stop failed (the earlier view stays on screen). */
  actionError: CommandError | null;
  /** What the person last asked for, so "Turn on again" can repeat it. */
  lastSpec: AwakeSpec | null;
}

/** Power: the agent's keep-awake view, kept current by what the agent pushes. The agent decides; this store only asks. */
export const PowerStore = signalStore(
  { providedIn: 'root' },
  withState<PowerState>({ view: null, error: null, busy: false, actionError: null, lastSpec: null }),
  withMethods((store, backend = inject(BACKEND)) => {
    async function act(call: () => Promise<PowerView>, spec: AwakeSpec | null): Promise<boolean> {
      patchState(store, { busy: true, actionError: null });
      try {
        patchState(store, { view: await call(), error: null, ...(spec ? { lastSpec: spec } : {}) });
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
          patchState(store, { view: await backend.powerState(), error: null });
        } catch (e) {
          patchState(store, { error: toCommandError(e) });
        }
      },
      start: (spec: AwakeSpec) => act(() => backend.powerKeepAwake(spec), spec),
      stop: () => act(() => backend.powerStopKeepAwake(), null),
      idleCancel: () => act(() => backend.powerIdleCancel(), null),
      idleSnooze: (minutes: number) => act(() => backend.powerIdleSnooze(minutes), null),
      receive(view: PowerView): void {
        patchState(store, { view, error: null });
      },
    };
  }),
  withHooks((store) => {
    const backend = inject(BACKEND);
    let stop: Unsubscribe | null = null;
    return {
      onInit() {
        stop = backend.onPower((v) => store.receive(v));
      },
      onDestroy() {
        stop?.();
      },
    };
  }),
);
