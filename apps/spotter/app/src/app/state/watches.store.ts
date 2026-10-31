import { computed, inject } from '@angular/core';
import { patchState, signalStore, withComputed, withMethods, withState } from '@ngrx/signals';
import type { CommandError, WatchSummary } from '../core/api.types';
import { BACKEND, type ReleaseTarget } from '../core/backend';
import { isCancelled, toCommandError } from '../core/command-error';
import { needsAttention } from '../shared/labels';
import { Notifier } from './notifier';

export type WatchFilter = 'all' | 'new' | 'attention' | 'paused';
export type WatchSort = 'name' | 'latest' | 'checked';

interface WatchesState {
  watches: WatchSummary[];
  loaded: boolean;
  loading: boolean;
  error: CommandError | null;
  query: string;
  filter: WatchFilter;
  sort: WatchSort;
}

/** The watch list, kept current by the backend's pushed state. Also the source of the tray's and the badge's counts. */
export const WatchesStore = signalStore(
  { providedIn: 'root' },
  withState<WatchesState>({ watches: [], loaded: false, loading: false, error: null, query: '', filter: 'all', sort: 'name' }),
  withComputed((s) => ({
    unseen: computed(() => s.watches().filter((w) => w.unseen)),
    attention: computed(() => s.watches().filter(needsAttention)),
    checking: computed(() => s.watches().some((w) => w.checking)),
    visible: computed(() => {
      const q = s.query().trim().toLowerCase();
      const f = s.filter();
      const list = s.watches().filter((w) => {
        if (q && !`${w.name} ${w.sourceLabel} ${w.sourceDescription} ${w.latest?.version ?? ''}`.toLowerCase().includes(q)) return false;
        if (f === 'new') return w.unseen;
        if (f === 'attention') return needsAttention(w);
        if (f === 'paused') return w.paused;
        return true;
      });
      const by = s.sort();
      return [...list].sort((a, b) => {
        if (by === 'latest') return (b.latest?.firstSeenMs ?? 0) - (a.latest?.firstSeenMs ?? 0);
        if (by === 'checked') return (b.lastCheckMs ?? 0) - (a.lastCheckMs ?? 0);
        return a.name.localeCompare(b.name);
      });
    }),
  })),
  withMethods((store) => {
    const backend = inject(BACKEND);
    const toast = inject(Notifier);
    let started = false;

    /** Runs a mutation; its failure becomes a toast and the list is refreshed from the backend either way. */
    const act = async (work: () => Promise<unknown>, success: string | null): Promise<boolean> => {
      try {
        await work();
        if (success) toast.success(success);
        return true;
      } catch (e) {
        toast.error('That did not work', toCommandError(e).message);
        return false;
      }
    };

    const load = async (): Promise<void> => {
      patchState(store, { loading: true });
      try {
        patchState(store, { watches: await backend.listWatches(), loaded: true, loading: false, error: null });
      } catch (e) {
        patchState(store, { loading: false, loaded: true, error: toCommandError(e) });
      }
    };

    return {
      load,
      /** Starts loading and subscribing in the background; never awaited before the first paint. */
      start(): void {
        if (started) return;
        started = true;
        void load();
        backend.onState((push) => patchState(store, { watches: push.watches, loaded: true, error: null })).catch(() => undefined);
      },
      setQuery: (query: string) => patchState(store, { query }),
      setFilter: (filter: WatchFilter) => patchState(store, { filter }),
      setSort: (sort: WatchSort) => patchState(store, { sort }),
      byId: (id: string) => store.watches().find((w) => w.id === id) ?? null,
      /**
       * Checks one watch (done when its check is, however long that takes) or every watch (starts them and returns:
       * "n of m" and Cancel are in the operations panel, the watches report through the pushed state).
       */
      async checkNow(id: string | null): Promise<boolean> {
        const name = id === null ? null : (store.watches().find((w) => w.id === id)?.name ?? 'the watch');
        try {
          if (name === null) {
            await backend.checkNow(null);
            toast.success('Checking every watch');
          } else {
            await backend.checkNow(id);
            toast.success('Check finished');
          }
          return true;
        } catch (e) {
          if (isCancelled(e)) toast.info('Check cancelled');
          else toast.error('That did not work', toCommandError(e).message);
          return false;
        }
      },
      setPaused: (id: string, paused: boolean) => act(() => backend.setPaused(id, paused), paused ? 'Watch paused' : 'Watch resumed'),
      acknowledge: (id: string, version: string | null) => act(() => backend.acknowledge(id, version), 'Marked as seen'),
      async acknowledgeAll(): Promise<void> {
        const unseen = store.watches().filter((w) => w.unseen);
        const results = await Promise.all(unseen.map((w) => act(() => backend.acknowledge(w.id, null), null)));
        if (results.every(Boolean)) toast.success(unseen.length === 1 ? 'Marked as seen' : `Marked ${unseen.length} watches as seen`);
      },
      setInstalled: (id: string, version: string | null) => act(() => backend.setInstalled(id, version), version ? `Recorded ${version} as installed` : 'Installed version cleared'),
      remove: (id: string) => act(() => backend.deleteWatch(id), 'Watch deleted'),
      acknowledgeProduct: (id: string, product: string) => act(() => backend.acknowledgeProduct(id, product), 'Marked as seen'),
      openRelease: (id: string, version: string, target?: ReleaseTarget) => act(() => backend.openRelease(id, version, target), null),
      async download(id: string, version: string, target?: ReleaseTarget): Promise<void> {
        try {
          const path = await backend.downloadRelease(id, version, target);
          toast.success('Download finished', path);
        } catch (e) {
          // Cancelling is the user's own choice, not a failure; the part stays and the next download resumes it.
          if (isCancelled(e)) toast.info('Download cancelled', 'The part already downloaded is kept; downloading again resumes it.');
          else toast.error('Download failed', toCommandError(e).message);
        }
      },
    };
  }),
);
