import { inject } from '@angular/core';
import { patchState, signalStore, withMethods, withState } from '@ngrx/signals';
import type { CommandError, HealthReport, StartupEntry, StartupState, WeeklyReport } from '../core/api.types';
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
          const [report, weekly] = await Promise.all([backend.health(), backend.weeklyReport().catch(() => null)]);
          patchState(store, { report, weekly });
        } catch (e) {
          patchState(store, { reportError: toCommandError(e) });
        }
      },
      async loadStartup(): Promise<void> {
        patchState(store, { startupError: null });
        try {
          patchState(store, { startup: await backend.startupEntries(), startupLoaded: true });
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
