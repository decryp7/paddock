import { inject } from '@angular/core';
import { patchState, signalStore, withMethods, withState } from '@ngrx/signals';
import type { AppEvent, Dashboard, DashboardRange } from '../core/api.types';
import { BACKEND } from '../core/backend';
import { idle, pending, settle, type Loadable } from './loadable';

/** How long pushes are collected before the dashboard is recomputed. */
export const RELOAD_DEBOUNCE_MS = 400;
export const EVENTS_PAGE = 20;

interface DashboardState {
  range: DashboardRange;
  dashboard: Loadable<Dashboard>;
  events: Loadable<AppEvent[]>;
  /** The last page was full, so there may be more. */
  moreEvents: boolean;
}

/** The dashboard numbers and the events feed. Queries only; nothing here changes watches. */
export const DashboardStore = signalStore(
  { providedIn: 'root' },
  withState<DashboardState>({ range: '30d', dashboard: idle<Dashboard>(), events: idle<AppEvent[]>(), moreEvents: false }),
  withMethods((store) => {
    const backend = inject(BACKEND);
    let timer: ReturnType<typeof setTimeout> | undefined;

    const loadDashboard = async (): Promise<void> => {
      patchState(store, { dashboard: pending(store.dashboard()) });
      patchState(store, { dashboard: await settle(store.dashboard(), () => backend.dashboard(store.range())) });
    };
    const loadEvents = async (): Promise<void> => {
      patchState(store, { events: pending(store.events()) });
      const next = await settle(store.events(), () => backend.events(EVENTS_PAGE, null));
      patchState(store, { events: next, moreEvents: (next.value?.length ?? 0) >= EVENTS_PAGE });
    };

    return {
      loadDashboard,
      loadEvents,
      load: () => Promise.all([loadDashboard(), loadEvents()]).then(() => undefined),
      setRange(range: DashboardRange): Promise<void> {
        patchState(store, { range });
        return loadDashboard();
      },
      /** Recomputes shortly after the last change (pushes arrive in bursts). */
      scheduleReload(): void {
        clearTimeout(timer);
        timer = setTimeout(() => void Promise.all([loadDashboard(), loadEvents()]), RELOAD_DEBOUNCE_MS);
      },
      async loadMoreEvents(): Promise<void> {
        const current = store.events().value ?? [];
        const before = current[current.length - 1]?.atMs ?? null;
        const next = await settle(store.events(), async () => {
          const page = await backend.events(EVENTS_PAGE, before);
          patchState(store, { moreEvents: page.length >= EVENTS_PAGE });
          return [...current, ...page];
        });
        patchState(store, { events: next });
      },
    };
  }),
);
