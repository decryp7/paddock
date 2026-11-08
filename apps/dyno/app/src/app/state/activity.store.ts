import { inject } from '@angular/core';
import { patchState, signalStore, withHooks, withMethods, withState } from '@ngrx/signals';
import type { ActivityEntry, CommandError } from '../core/api.types';
import { EVENTS_KEPT } from './events-kept';
import { BACKEND, type Unsubscribe } from '../core/backend';
import { toCommandError } from '../core/command-error';

interface ActivityState {
  entries: ActivityEntry[];
  loaded: boolean;
  loading: boolean;
  error: CommandError | null;
}

/** Puts a pushed or changed entry into the list: replaces the one with its id, else adds it on top. */
export function mergeEntry(entries: readonly ActivityEntry[], entry: ActivityEntry, limit = EVENTS_KEPT): ActivityEntry[] {
  const i = entries.findIndex((e) => e.id === entry.id);
  if (i >= 0) return entries.map((e, j) => (j === i ? entry : e));
  return [entry, ...entries].sort((a, b) => b.at - a.at).slice(0, limit);
}

/** What Dyno noticed (alerts, its own notes), newest first, kept current by pushed entries. */
export const ActivityStore = signalStore(
  { providedIn: 'root' },
  withState<ActivityState>({ entries: [], loaded: false, loading: false, error: null }),
  withMethods((store, backend = inject(BACKEND)) => ({
    async load(limit = 100): Promise<void> {
      patchState(store, { loading: true, error: null });
      try {
        const entries = await backend.activity(limit);
        patchState(store, { entries, loaded: true, loading: false });
      } catch (e) {
        patchState(store, { loading: false, error: toCommandError(e) });
      }
    },
    receive(entry: ActivityEntry): void {
      patchState(store, (s) => ({ entries: mergeEntry(s.entries, entry) }));
    },
  })),
  withHooks((store) => {
    const backend = inject(BACKEND);
    let stop: Unsubscribe | null = null;
    return {
      onInit() {
        stop = backend.onActivity((e) => store.receive(e));
      },
      onDestroy() {
        stop?.();
      },
    };
  }),
);
