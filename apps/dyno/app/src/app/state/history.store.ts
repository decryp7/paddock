import { inject } from '@angular/core';
import { patchState, signalStore, withMethods, withState } from '@ngrx/signals';
import type { CommandError, History, HistoryRange } from '../core/api.types';
import { BACKEND } from '../core/backend';
import { toCommandError } from '../core/command-error';

interface HistoryState {
  range: HistoryRange;
  history: History | null;
  loading: boolean;
  error: CommandError | null;
}

/** The stored history of the range the dashboard shows (the agent keeps ~24 h raw, then 1 min / 15 min / hourly roll-ups). */
export const HistoryStore = signalStore(
  { providedIn: 'root' },
  withState<HistoryState>({ range: 'day', history: null, loading: false, error: null }),
  withMethods((store, backend = inject(BACKEND)) => {
    /** Only the answer to the newest request is kept (a slow answer for an old range must not overwrite it). */
    let latest = 0;
    return {
      async load(): Promise<void> {
        const ticket = ++latest;
        patchState(store, { loading: true, error: null });
        try {
          const history = await backend.history(store.range(), null);
          if (ticket === latest) patchState(store, { history, loading: false });
        } catch (e) {
          if (ticket === latest) patchState(store, { loading: false, error: toCommandError(e) });
        }
      },
      async setRange(range: HistoryRange): Promise<void> {
        if (range === store.range() && store.history()) return;
        patchState(store, { range, history: store.range() === range ? store.history() : null });
        await this.load();
      },
      /** Loads once (the dashboard and the tray call it freely). */
      async ensure(): Promise<void> {
        if (!store.history() && !store.loading()) await this.load();
      },
    };
  }),
);
