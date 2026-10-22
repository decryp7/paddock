import { inject } from '@angular/core';
import { patchState, signalStore, withHooks, withMethods, withState } from '@ngrx/signals';
import type { ActivityEntry, CommandError } from '../core/api.types';
import { BACKEND, type Unsubscribe } from '../core/backend';
import { toCommandError } from '../core/command-error';
import { Notifier } from '../shared/notifier';

interface ActivityState {
  entries: ActivityEntry[];
  loaded: boolean;
  loading: boolean;
  error: CommandError | null;
  /** Entries being undone right now. */
  undoing: string[];
}

/** Puts a pushed or changed entry into the list: replaces the one with its id, else adds it on top. */
export function mergeEntry(entries: readonly ActivityEntry[], entry: ActivityEntry, limit = 200): ActivityEntry[] {
  const i = entries.findIndex((e) => e.id === entry.id);
  if (i >= 0) return entries.map((e, j) => (j === i ? entry : e));
  return [entry, ...entries].sort((a, b) => b.at - a.at).slice(0, limit);
}

/** What Dyno did (rules, cleanups, tuning, your actions), newest first, kept current by pushed entries. */
export const ActivityStore = signalStore(
  { providedIn: 'root' },
  withState<ActivityState>({ entries: [], loaded: false, loading: false, error: null, undoing: [] }),
  withMethods((store, backend = inject(BACKEND), notifier = inject(Notifier)) => ({
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
    /** Undoes an entry; the result is logged as a new entry and the original shows "Undone". */
    async undo(id: string): Promise<boolean> {
      patchState(store, (s) => ({ undoing: [...s.undoing, id] }));
      try {
        const result = await backend.undo(id);
        patchState(store, (s) => ({
          entries: mergeEntry(
            s.entries.map((e) => (e.id === id ? { ...e, undo: 'undone' as const } : e)),
            result,
          ),
        }));
        notifier.show(result.title);
        return true;
      } catch (e) {
        notifier.error(toCommandError(e));
        return false;
      } finally {
        patchState(store, (s) => ({ undoing: s.undoing.filter((x) => x !== id) }));
      }
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
