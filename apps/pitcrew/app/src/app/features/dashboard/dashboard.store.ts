import { inject } from '@angular/core';
import { patchState, signalStore, withMethods, withState } from '@ngrx/signals';
import type { ActivityReply, MetricsReply, RunDetails } from '../../core/api.types';
import { BACKEND } from '../../core/backend';
import { readLocal, writeLocal } from '../../core/storage';
import { idle, pending, settle, type Loadable } from '../../state/loadable';

/** The history range the dashboard shows. */
export type Range = '24h' | '7d' | '30d';

/** Days of activity and hours of metrics per range ("24 h" shows yesterday and today as days). */
export const RANGES: Record<Range, { days: number; hours: number; label: string }> = {
  '24h': { days: 2, hours: 24, label: '24 h' },
  '7d': { days: 7, hours: 168, label: '7 days' },
  '30d': { days: 30, hours: 720, label: '30 days' },
};

/** Refreshes of history and metrics happen at most this often (plus on range changes). */
export const REFRESH_MS = 60_000;
const RANGE_KEY = 'pitcrew.dashboard.range';

function storedRange(): Range {
  const v = readLocal(RANGE_KEY);
  return v === '24h' || v === '7d' || v === '30d' ? v : '7d';
}

interface DashboardState {
  range: Range;
  /** `null` = all runners. */
  focus: string | null;
  activity: Record<string, Loadable<ActivityReply>>;
  machine: Loadable<MetricsReply>;
  folders: Record<string, Loadable<MetricsReply>>;
  details: Record<string, RunDetails | null>;
  lastRefresh: number;
}

/** History, metrics and current-run details for the dashboard; fetched on demand, never polled tightly. */
export const DashboardStore = signalStore(
  withState<DashboardState>(() => ({ range: storedRange(), focus: null, activity: {}, machine: idle<MetricsReply>(), folders: {}, details: {}, lastRefresh: 0 })),
  withMethods((store, backend = inject(BACKEND)) => {
    let runnerNames: string[] = [];

    const loadAll = async (): Promise<void> => {
      const { days, hours } = RANGES[store.range()];
      patchState(store, {
        lastRefresh: Date.now(),
        machine: pending(store.machine()),
        activity: Object.fromEntries(runnerNames.map((n) => [n, pending(store.activity()[n])])),
        folders: Object.fromEntries(runnerNames.map((n) => [n, pending(store.folders()[n])])),
      });
      await Promise.all([
        settle(store.machine(), () => backend.metrics(null, hours)).then((machine) => patchState(store, { machine })),
        ...runnerNames.map(async (n) => {
          const a = await settle(store.activity()[n], () => backend.activity(n, days));
          patchState(store, { activity: { ...store.activity(), [n]: a } });
        }),
        ...runnerNames.map(async (n) => {
          const f = await settle(store.folders()[n], () => backend.metrics(n, hours));
          patchState(store, { folders: { ...store.folders(), [n]: f } });
        }),
      ]);
    };

    return {
      /** Sets the runners to show; loads when the set changed, else at most every {@link REFRESH_MS}. */
      async refresh(names: string[], force = false): Promise<void> {
        const changed = names.join('\n') !== runnerNames.join('\n');
        runnerNames = [...names];
        if (!force && !changed && Date.now() - store.lastRefresh() < REFRESH_MS) return;
        await loadAll();
      },
      async setRange(range: Range): Promise<void> {
        if (range === store.range()) return;
        patchState(store, { range });
        writeLocal(RANGE_KEY, range);
        await loadAll();
      },
      setFocus(focus: string | null): void {
        patchState(store, { focus });
      },
      /** Fetches the current run's details (title, workflow, link) for a runner's job. */
      async loadDetails(runner: string): Promise<void> {
        try {
          const d = await backend.runDetails(runner);
          patchState(store, { details: { ...store.details(), [runner]: d } });
        } catch {
          patchState(store, { details: { ...store.details(), [runner]: null } });
        }
      },
      retry(): Promise<void> {
        return loadAll();
      },
    };
  }),
);
