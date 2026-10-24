import { computed, inject } from '@angular/core';
import { patchState, signalStore, withComputed, withHooks, withMethods, withState } from '@ngrx/signals';
import { SHELL_PORTS } from '@decryp7/paddock-ui/shell';
import { errorMessage, setError, setLoaded, setLoading, withRequestStatus } from '@decryp7/paddock-ui/shell/store';
import { ACTIVITY_PAGE_SIZE, ActivityBackend, type Unlisten } from './activity-backend';
import type { ActivityEntry } from './activity-wire';

export const ACTIVITY_NOTICE_KIND = 'pd.activity.undoFailed';

interface ActivityState {
  /** Newest first. */
  entries: readonly ActivityEntry[];
  /** Entries being undone right now. */
  undoing: readonly number[];
}

/** Puts a pushed or changed entry into the list: replaces the one with its id, else adds it on top; the oldest drop off. */
export function mergeEntry(entries: readonly ActivityEntry[], entry: ActivityEntry, limit = ACTIVITY_PAGE_SIZE): ActivityEntry[] {
  const known = entries.some((e) => e.id === entry.id);
  const merged = known ? entries.map((e) => (e.id === entry.id ? entry : e)) : [entry, ...entries];
  return merged.sort((a, b) => b.id - a.id).slice(0, limit);
}

/**
 * What the app did on the user's behalf, newest first, kept current by the journal's events (`paddock_jobs::journal`).
 * The engine is the single source of truth: this keeps the newest copy of each entry, the request status of the list and
 * which entries are being undone. Provided per window by `provideShellActivity`.
 */
export const ActivityStore = signalStore(
  withState<ActivityState>({ entries: [], undoing: [] }),
  withRequestStatus(),
  withComputed((store) => ({
    undoableCount: computed(() => store.entries().filter((e) => e.undo === 'available').length),
  })),
  withMethods((store, backend = inject(ActivityBackend), ports = inject(SHELL_PORTS)) => ({
    /** Reads the list (also the Retry of a failed load). */
    async load(): Promise<void> {
      patchState(store, setLoading());
      try {
        const entries = await backend.list(ACTIVITY_PAGE_SIZE);
        patchState(store, (s) => ({ entries: entries.reduce((all, e) => mergeEntry(all, e), s.entries as ActivityEntry[]), ...setLoaded() }));
      } catch (e) {
        patchState(store, setError(errorMessage(e, $localize`:@@pd.activity.noAnswer:The activity log did not answer.`)));
      }
    },

    /** An entry from the journal's event. */
    _receive(entry: ActivityEntry): void {
      patchState(store, (s) => ({ entries: mergeEntry(s.entries, entry) }));
    },

    /** Undoes an entry through the app. A refusal is a notice with the app's reason; the entry stays as it was. */
    async undo(id: number): Promise<boolean> {
      if (store.undoing().includes(id)) return false;
      patchState(store, (s) => ({ undoing: [...s.undoing, id] }));
      try {
        const entry = await backend.undo(id);
        patchState(store, (s) => ({ entries: mergeEntry(s.entries, entry) }));
        return true;
      } catch (e) {
        ports.notify({ level: 'error', kind: ACTIVITY_NOTICE_KIND, message: errorMessage(e, $localize`:@@pd.activity.undoFailed:The action could not be undone.`) });
        return false;
      } finally {
        patchState(store, (s) => ({ undoing: s.undoing.filter((x) => x !== id) }));
      }
    },
  })),
  withHooks((store, backend = inject(ActivityBackend)) => {
    let off: Unlisten | undefined;
    let destroyed = false;
    return {
      onInit() {
        // Listen first, then read the list: an entry during the read is merged, none is missed.
        void backend
          .listen((entry) => store._receive(entry))
          .then((u) => (destroyed ? u() : (off = u)))
          .catch((e: unknown) => console.warn(`[paddock-activity] cannot listen for activity: ${errorMessage(e, 'unknown')}`));
        void store.load();
      },
      onDestroy() {
        destroyed = true;
        off?.();
      },
    };
  }),
);

export type ActivityStoreType = InstanceType<typeof ActivityStore>;
