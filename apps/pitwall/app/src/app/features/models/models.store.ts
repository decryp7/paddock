import { computed, inject } from '@angular/core';
import { patchState, signalStore, withComputed, withMethods, withState } from '@ngrx/signals';
import type { CommandError, DiskUsage, HfModel, HfRepo, LocalModel, QuantVariant } from '../../core/api.types';
import { BACKEND } from '../../core/backend';
import { toCommandError } from '../../core/command-error';
import { Toasts } from '../../core/toasts';
import { bytes } from '../../shared/format';

interface ModelsState {
  library: LocalModel[];
  disk: DiskUsage | null;
  loading: boolean;
  loaded: boolean;
  error: CommandError | null;
  query: string;
  results: HfModel[];
  searching: boolean;
  searched: boolean;
  searchError: CommandError | null;
  repoName: string | null;
  repo: HfRepo | null;
  repoLoading: boolean;
  repoError: CommandError | null;
  /** Actions in flight, by key (`model:<id>`, `download:<file>`, `cancel:<id>`, `cleanup`). */
  busy: Record<string, true>;
}

const initial: ModelsState = {
  library: [],
  disk: null,
  loading: false,
  loaded: false,
  error: null,
  query: '',
  results: [],
  searching: false,
  searched: false,
  searchError: null,
  repoName: null,
  repo: null,
  repoLoading: false,
  repoError: null,
  busy: {},
};

/** The Models screen: the local library with disk usage, Hugging Face search and a repo's quantisation variants. */
export const ModelsStore = signalStore(
  withState(initial),
  withComputed(({ library, disk, busy }) => ({
    unusedModels: computed(() => {
      const ids = new Set(disk()?.unused ?? []);
      return library().filter((m) => ids.has(m.id));
    }),
    unusedBytes: computed(() => {
      const ids = new Set(disk()?.unused ?? []);
      return library()
        .filter((m) => ids.has(m.id))
        .reduce((s, m) => s + m.sizeBytes, 0);
    }),
    isBusy: computed(() => (key: string) => key in busy()),
  })),
  withMethods((store, backend = inject(BACKEND), toasts = inject(Toasts)) => {
    const setBusy = (key: string, on: boolean) => {
      const next = { ...store.busy() };
      if (on) next[key] = true;
      else delete next[key];
      patchState(store, { busy: next });
    };

    /** Runs an action with a busy flag; a failure becomes a toast and the last good data stays. */
    async function act(key: string, fn: () => Promise<unknown>, ok?: string): Promise<boolean> {
      if (key in store.busy()) return false;
      setBusy(key, true);
      try {
        await fn();
        if (ok) toasts.ok(ok);
        return true;
      } catch (e) {
        toasts.error(toCommandError(e));
        return false;
      } finally {
        setBusy(key, false);
      }
    }

    async function load(): Promise<void> {
      patchState(store, { loading: true, error: null });
      try {
        const [library, disk] = await Promise.all([backend.localModels(), backend.diskUsage()]);
        patchState(store, { library, disk, loading: false, loaded: true });
      } catch (e) {
        patchState(store, { loading: false, error: toCommandError(e) });
      }
    }

    return {
      load,

      async search(query: string): Promise<void> {
        patchState(store, { query, searching: true, searchError: null });
        try {
          const results = await backend.searchModels(query);
          if (store.query() === query) patchState(store, { results, searching: false, searched: true });
        } catch (e) {
          if (store.query() === query) patchState(store, { searching: false, searchError: toCommandError(e) });
        }
      },

      async openRepo(repo: string): Promise<void> {
        patchState(store, { repoName: repo, repo: null, repoLoading: true, repoError: null });
        try {
          const data = await backend.repoFiles(repo);
          if (store.repoName() === repo) patchState(store, { repo: data, repoLoading: false });
        } catch (e) {
          if (store.repoName() === repo) patchState(store, { repoLoading: false, repoError: toCommandError(e) });
        }
      },

      closeRepo(): void {
        patchState(store, { repoName: null, repo: null, repoError: null, repoLoading: false });
      },

      /** Starts a download (the projector of a vision repo comes along); progress arrives as live updates. */
      download(variant: QuantVariant, projector: string | null): Promise<boolean> {
        const repo = store.repoName();
        if (!repo) return Promise.resolve(false);
        return act(`download:${variant.file}`, () => backend.downloadModel(repo, variant.file, projector), `Downloading ${variant.file}.`);
      },

      cancelDownload(id: string): Promise<boolean> {
        return act(`cancel:${id}`, () => backend.cancelDownload(id), 'Download cancelled.');
      },

      async loadModel(id: string): Promise<void> {
        if (await act(`model:${id}`, () => backend.loadModel(id))) await load();
      },

      async unloadModel(id: string): Promise<void> {
        if (await act(`model:${id}`, () => backend.unloadModel(id))) await load();
      },

      async pinModel(id: string, pinned: boolean): Promise<void> {
        const ok = await act(`model:${id}`, () => backend.pinModel(id, pinned), pinned ? 'Pinned: stays loaded until you unpin it.' : 'Unpinned.');
        if (ok) patchState(store, { library: store.library().map((m) => (m.id === id ? { ...m, pinned } : m)) });
      },

      /** Sets the per-model idle release (null: the global setting). */
      async setIdle(id: string, idleSecs: number | null): Promise<void> {
        const ok = await act(`model:${id}`, () => backend.setModelIdle(id, idleSecs), idleSecs === null ? 'Uses the global idle time again.' : 'Idle time saved.');
        if (ok) patchState(store, { library: store.library().map((m) => (m.id === id ? { ...m, idleSecs } : m)) });
      },

      async deleteModel(id: string): Promise<void> {
        const name = store.library().find((m) => m.id === id)?.name ?? id;
        if (await act(`model:${id}`, () => backend.deleteModel(id), `Deleted ${name}.`)) await load();
      },

      /** Deletes every model unused for the configured days (loaded ones are never unused). */
      async cleanUnused(): Promise<void> {
        const models = store.unusedModels();
        if (!models.length) return;
        const freed = store.unusedBytes();
        await act(
          'cleanup',
          async () => {
            for (const m of models) await backend.deleteModel(m.id);
          },
          `Freed ${bytes(freed)}.`,
        );
        // Reload either way: a failure part-way still deleted the models before it.
        await load();
      },
    };
  }),
);

export type ModelsStore = InstanceType<typeof ModelsStore>;
