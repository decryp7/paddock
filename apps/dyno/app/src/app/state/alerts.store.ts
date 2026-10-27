import { inject } from '@angular/core';
import { patchState, signalStore, withHooks, withMethods, withState } from '@ngrx/signals';
import type { Alert, CommandError } from '../core/api.types';
import { BACKEND, type Unsubscribe } from '../core/backend';
import { toCommandError } from '../core/command-error';

interface AlertsState {
  alerts: Alert[];
  loaded: boolean;
  loading: boolean;
  error: CommandError | null;
}

/** Puts a pushed alert on top of the list (replacing one with its id), newest first, capped. */
export function mergeAlert(alerts: readonly Alert[], alert: Alert, limit = 100): Alert[] {
  return [alert, ...alerts.filter((a) => a.id !== alert.id)].sort((a, b) => b.at - a.at).slice(0, limit);
}

/** The recent alerts (low disk, overheating, ...), kept current by pushed alerts. */
export const AlertsStore = signalStore(
  { providedIn: 'root' },
  withState<AlertsState>({ alerts: [], loaded: false, loading: false, error: null }),
  withMethods((store, backend = inject(BACKEND)) => ({
    async load(limit = 50): Promise<void> {
      patchState(store, { loading: true, error: null });
      try {
        patchState(store, { alerts: await backend.alerts(limit), loaded: true, loading: false });
      } catch (e) {
        patchState(store, { loading: false, error: toCommandError(e) });
      }
    },
    receive(alert: Alert): void {
      patchState(store, (s) => ({ alerts: mergeAlert(s.alerts, alert) }));
    },
  })),
  withHooks((store) => {
    const backend = inject(BACKEND);
    let stop: Unsubscribe | null = null;
    return {
      onInit() {
        stop = backend.onAlert((a) => store.receive(a));
      },
      onDestroy() {
        stop?.();
      },
    };
  }),
);
