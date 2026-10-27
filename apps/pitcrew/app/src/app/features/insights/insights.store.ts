import { inject } from '@angular/core';
import { patchState, signalStore, withMethods, withState } from '@ngrx/signals';
import type { InsightsReply } from '../../core/api.types';
import { BACKEND } from '../../core/backend';
import { idle, pending, settle, type Loadable } from '../../state/loadable';

export const INSIGHTS_DAYS = [7, 14, 30, 90] as const;
export type InsightsDays = (typeof INSIGHTS_DAYS)[number];

interface InsightsState {
  days: InsightsDays;
  reply: Loadable<InsightsReply>;
}

/** Workflow insights for the selected range; the engine computes everything, the store only loads it. */
export const InsightsStore = signalStore(
  withState<InsightsState>({ days: 30, reply: idle<InsightsReply>() }),
  withMethods((store, backend = inject(BACKEND)) => {
    const load = async () => {
      patchState(store, { reply: pending(store.reply()) });
      const reply = await settle(store.reply(), () => backend.insights(store.days()));
      patchState(store, { reply });
    };
    return {
      load,
      async setDays(days: InsightsDays): Promise<void> {
        if (days === store.days() && store.reply().value) return;
        patchState(store, { days });
        await load();
      },
      retry: load,
    };
  }),
);
