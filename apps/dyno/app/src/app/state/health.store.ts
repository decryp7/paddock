import { inject } from '@angular/core';
import { patchState, signalStore, withMethods, withState } from '@ngrx/signals';
import type { CommandError, HealthReport, StartupEntry, StartupFilter, StartupState, WeeklyReport } from '../core/api.types';
import { BACKEND } from '../core/backend';
import { toCommandError } from '../core/command-error';
import { Notifier } from '../shared/notifier';
import { ActivityStore } from './activity.store';

interface HealthState {
  report: HealthReport | null;
  reportError: CommandError | null;
  /** Last week's report, for the score trend. */
  weekly: WeeklyReport | null;
  startup: StartupEntry[];
  /** Every entry before the filter, and how many are the OS's and how many signed (the toggles show them). */
  startupCounts: { total: number; os: number; signed: number };
  startupLoaded: boolean;
  startupError: CommandError | null;
  /** Ids of startup entries with a change in flight. */
  busy: string[];
}

const initial: HealthState = {
  report: null,
  reportError: null,
  weekly: null,
  startup: [],
  startupCounts: { total: 0, os: 0, signed: 0 },
  startupLoaded: false,
  startupError: null,
  busy: [],
};

/** The health score with its reasons, and the startup manager. */
export const HealthStore = signalStore(
  { providedIn: 'root' },
  withState(initial),
  withMethods((store, backend = inject(BACKEND), notifier = inject(Notifier), activity = inject(ActivityStore)) => {
    const busy = (id: string, on: boolean) => patchState(store, (s) => ({ busy: on ? [...s.busy, id] : s.busy.filter((x) => x !== id) }));
    return {
      async loadReport(): Promise<void> {
        patchState(store, { reportError: null });
        try {
          const [report, weekly] = await Promise.all([backend.health(), backend.weeklyReport(0).catch(() => null)]);
          patchState(store, { report, weekly });
        } catch (e) {
          patchState(store, { reportError: toCommandError(e) });
        }
      },
      /** Loads the startup list with the engine's filter applied (`hideOs`, `hideSigned`) and the counts. */
      async loadStartup(filter: StartupFilter = { hideOs: false, hideSigned: false }): Promise<void> {
        patchState(store, { startupError: null });
        try {
          const o = await backend.startupOverview(filter);
          patchState(store, { startup: o.entries, startupCounts: { total: o.total, os: o.osCount, signed: o.signedCount }, startupLoaded: true });
        } catch (e) {
          patchState(store, { startupError: toCommandError(e) });
        }
      },
      async setStartup(id: string, state: StartupState): Promise<boolean> {
        busy(id, true);
        try {
          const entry = await backend.setStartupState(id, state);
          activity.receive(entry);
          patchState(store, (s) => ({ startup: s.startup.map((e) => (e.id === id ? { ...e, state } : e)) }));
          notifier.show(entry.title);
          return true;
        } catch (e) {
          notifier.error(toCommandError(e));
          return false;
        } finally {
          busy(id, false);
        }
      },
    };
  }),
);
