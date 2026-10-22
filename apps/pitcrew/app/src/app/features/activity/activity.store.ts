import { inject } from '@angular/core';
import { patchState, signalStore, withMethods, withState } from '@ngrx/signals';
import type { ActivityReply } from '../../core/api.types';
import { BACKEND } from '../../core/backend';
import { pending, settle, type Loadable } from '../../state/loadable';

export const ACTIVITY_DAYS = [1, 7, 30] as const;
export type ActivityDays = (typeof ACTIVITY_DAYS)[number];

interface ActivityState {
  days: ActivityDays;
  activity: Record<string, Loadable<ActivityReply>>;
}

/** Activity history for the selected runners and range. */
export const ActivityStore = signalStore(
  withState<ActivityState>({ days: 7, activity: {} }),
  withMethods((store, backend = inject(BACKEND)) => {
    let names: string[] = [];
    const load = async () => {
      const days = store.days();
      patchState(store, { activity: Object.fromEntries(names.map((n) => [n, pending(store.activity()[n])])) });
      await Promise.all(
        names.map(async (n) => {
          const a = await settle(store.activity()[n], () => backend.activity(n, days));
          patchState(store, { activity: { ...store.activity(), [n]: a } });
        }),
      );
    };
    return {
      async show(runners: string[]): Promise<void> {
        if (runners.join('\n') === names.join('\n')) return;
        names = [...runners];
        await load();
      },
      async setDays(days: ActivityDays): Promise<void> {
        patchState(store, { days });
        await load();
      },
      retry: load,
    };
  }),
);
