import { inject } from '@angular/core';
import { patchState, signalStore, withMethods, withState } from '@ngrx/signals';
import type { AppDetail, CommandError, UsageRange, UsageSeries } from '../../core/api.types';
import { BACKEND } from '../../core/backend';
import { toCommandError } from '../../core/command-error';

interface UsageState {
  range: UsageRange;
  /** `null`: all apps together (the dashboard). */
  appId: string | null;
  series: UsageSeries | null;
  /** The app's detail (limits and latest requests); only loaded when an app is selected. */
  detail: AppDetail | null;
  loading: boolean;
  error: CommandError | null;
}

const initial: UsageState = { range: '24h', appId: null, series: null, detail: null, loading: false, error: null };

/**
 * Usage over time for the dashboard (all apps) or one app (with its detail). The server computes the buckets, totals
 * and limit statuses; this store only fetches them. A late answer for a range or app that is no longer selected is
 * ignored, so quick switching never shows the wrong chart.
 */
export const UsageStore = signalStore(
  withState(initial),
  withMethods((store, backend = inject(BACKEND)) => {
    async function fetchNow(): Promise<void> {
      const { range, appId } = store;
      const [r, a] = [range(), appId()];
      patchState(store, { loading: true, error: null });
      try {
        const [series, detail] = await Promise.all([backend.usageSeries(r, a), a === null ? Promise.resolve(null) : backend.appDetail(a)]);
        if (store.range() === r && store.appId() === a) patchState(store, { series, detail, loading: false });
      } catch (e) {
        if (store.range() === r && store.appId() === a) patchState(store, { loading: false, error: toCommandError(e) });
      }
    }
    return {
      /** Shows `appId` (or all apps) over `range`; the previous chart stays until the new one arrives. */
      async show(appId: string | null, range: UsageRange = store.range()): Promise<void> {
        if (appId !== store.appId()) patchState(store, { series: null, detail: null });
        patchState(store, { appId, range });
        await fetchNow();
      },
      async select(range: UsageRange): Promise<void> {
        patchState(store, { range });
        await fetchNow();
      },
      async reload(): Promise<void> {
        await fetchNow();
      },
    };
  }),
);

export type UsageStore = InstanceType<typeof UsageStore>;
