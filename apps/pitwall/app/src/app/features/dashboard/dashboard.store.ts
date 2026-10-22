import { computed, inject } from '@angular/core';
import { patchState, signalStore, withComputed, withMethods, withState } from '@ngrx/signals';
import type { CommandError, History, HistoryPoint, HistoryRange } from '../../core/api.types';
import { BACKEND } from '../../core/backend';
import { toCommandError } from '../../core/command-error';
import { LiveStore } from '../../core/live.store';

interface DashboardState {
  range: HistoryRange;
  histories: Partial<Record<HistoryRange, History>>;
  loading: boolean;
  error: CommandError | null;
}

const initial: DashboardState = { range: '1h', histories: {}, loading: false, error: null };

/** The 1 h chart continues with the live tail (points pushed after the history was fetched), within the hour. */
export function withTail(history: History, tail: HistoryPoint[]): HistoryPoint[] {
  if (history.range !== '1h' || tail.length === 0) return history.points;
  const last = history.points[history.points.length - 1]?.at ?? 0;
  const fresh = tail.filter((p) => p.at > last);
  if (fresh.length === 0) return history.points;
  const end = fresh[fresh.length - 1]!.at;
  const from = end - 3_600_000;
  return [...history.points.filter((p) => p.at >= from), ...fresh];
}

/** The dashboard's history charts: the selected range, cached per range, joined with the live tail. */
export const DashboardStore = signalStore(
  withState(initial),
  withComputed(({ range, histories }, live = inject(LiveStore)) => {
    const history = computed(() => histories()[range()] ?? null);
    return {
      history,
      points: computed(() => {
        const h = history();
        return h ? withTail(h, live.tail()) : [];
      }),
      /** History events plus live ones newer than the history. */
      chartEvents: computed(() => {
        const h = history();
        if (!h) return [];
        const seen = new Set(h.events.map((e) => e.id));
        const last = h.points[h.points.length - 1]?.at ?? 0;
        return [...h.events, ...live.events().filter((e) => !seen.has(e.id) && e.at > last)];
      }),
    };
  }),
  withMethods((store, backend = inject(BACKEND)) => ({
    /** Selects a range and fetches its history (a cached range shows at once and refreshes in place). */
    async select(range: HistoryRange): Promise<void> {
      patchState(store, { range, loading: true, error: null });
      try {
        const h = await backend.history(range);
        patchState(store, (s) => ({ histories: { ...s.histories, [range]: h }, loading: s.range === range ? false : s.loading }));
      } catch (e) {
        if (store.range() === range) patchState(store, { loading: false, error: toCommandError(e) });
      }
    },
  })),
);

export type DashboardStore = InstanceType<typeof DashboardStore>;
