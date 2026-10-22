import { computed, inject } from '@angular/core';
import { patchState, signalStore, withComputed, withMethods, withState } from '@ngrx/signals';
import type { CommandError, DiskUsage, EnginePinState, HfModel, HfRepo, InstalledBuild, LocalModel, PackEngine, PacksInfo, PinChange, QuantVariant, Recommendation, SlotsChange } from '../../core/api.types';
import { BACKEND } from '../../core/backend';
import { toCommandError } from '../../core/command-error';
import { Toasts } from '../../core/toasts';
import { bytes, slotsText } from '../../shared/format';

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
  /** Catalog recommendations and pack notes for this machine (filtered by `packTag`). */
  packs: PacksInfo | null;
  packsLoading: boolean;
  packsError: CommandError | null;
  packTag: string | null;
  /** Every tag seen in the unfiltered recommendations (the filter chips). */
  packTags: string[];
  /** Ids of the pack engines that have an installed build, and every loaded pack engine. */
  installedPacks: string[];
  packEngines: PackEngine[];
  /** The llama.cpp builds installed and the engine pins, for each model's pin menu (secondary, like the pack lists). */
  llamaBuilds: InstalledBuild[];
  enginePins: EnginePinState | null;
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
  packs: null,
  packsLoading: false,
  packsError: null,
  packTag: null,
  packTags: [],
  installedPacks: [],
  packEngines: [],
  llamaBuilds: [],
  enginePins: null,
  busy: {},
};

/** The Models screen: the local library with disk usage, Hugging Face search and a repo's quantisation variants. */
export const ModelsStore = signalStore(
  withState(initial),
  withComputed(({ library, disk, busy, packEngines, installedPacks, llamaBuilds, enginePins }) => ({
    /** Engines a model can be pinned to besides llama.cpp: pack engines that are installed. */
    engineChoices: computed<PackEngine[]>(() => {
      const have = new Set(installedPacks());
      return packEngines().filter((e) => have.has(e.id));
    }),
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
    /** Builds a model can be pinned to: installed and passed their self-test. */
    pinnableBuilds: computed(() => llamaBuilds().filter((b) => b.selfTest === 'passed')),
    /** A model's pin, with the build's release tag. */
    pinOf: computed(() => (modelId: string) => enginePins()?.models.find((p) => p.modelId === modelId) ?? null),
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
    async function act<T>(key: string, fn: () => Promise<T>, ok?: string | ((result: T) => string)): Promise<boolean> {
      if (key in store.busy()) return false;
      setBusy(key, true);
      try {
        const result = await fn();
        if (ok) toasts.ok(typeof ok === 'string' ? ok : ok(result));
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

    async function loadPacks(tag: string | null = store.packTag()): Promise<void> {
      patchState(store, { packsLoading: true, packsError: null, packTag: tag });
      try {
        const packs = await backend.packs(tag);
        if (store.packTag() !== tag) return;
        patchState(store, { packs, packsLoading: false, ...(tag === null ? { packTags: [...new Set(packs.recommendations.flatMap((r) => r.tags))].sort() } : {}) });
      } catch (e) {
        if (store.packTag() === tag) patchState(store, { packsLoading: false, packsError: toCommandError(e) });
      }
    }

    /** Which pack engines have a build installed (secondary: a failure leaves llama.cpp as the only choice). */
    async function loadInstalledPacks(): Promise<void> {
      try {
        const [engines, packs] = await Promise.all([backend.engines(), backend.packs()]);
        patchState(store, {
          installedPacks: engines.filter((e) => e.kind === 'pack').flatMap((e) => e.builds.map((b) => b.id)),
          packEngines: packs.engines,
          llamaBuilds: engines.find((e) => e.kind === 'llama.cpp')?.builds ?? [],
        });
      } catch {
        patchState(store, { installedPacks: [], packEngines: [], llamaBuilds: [] });
      }
      try {
        patchState(store, { enginePins: await backend.enginePins() });
      } catch {
        patchState(store, { enginePins: null });
      }
    }

    return {
      load,
      loadPacks,
      loadInstalledPacks,

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

      /**
       * One-click download of a recommendation: looks the repo up for the exact file (and the vision projector of a
       * vision model) and starts the download; progress arrives as live updates.
       */
      downloadRecommendation(rec: Recommendation): Promise<boolean> {
        return act(
          `rec:${rec.id}`,
          async () => {
            const repo = await backend.repoFiles(rec.repo);
            const file = repo.variants.find((v) => v.file === rec.file)?.file ?? rec.file;
            const projector = rec.tags.includes('vision') ? (repo.projectors[0]?.file ?? null) : null;
            await backend.downloadModel(rec.repo, file, projector);
          },
          `Downloading ${rec.name}.`,
        );
      },

      /** Chooses the engine for one model: a pack engine id, or null for llama.cpp. */
      async setEngine(id: string, engine: string | null): Promise<void> {
        const ok = await act(`model:${id}`, () => backend.setModelEngine(id, engine), engine === null ? 'Uses llama.cpp.' : 'Engine saved. The model uses it from its next load.');
        if (ok) patchState(store, { library: store.library().map((m) => (m.id === id ? { ...m, engine } : m)) });
      },

      /** Pins a model to an installed llama.cpp build (null: follows the global choice). A loaded model keeps its engine until its next load; the toast says so. */
      async setPin(id: string, buildId: string | null): Promise<void> {
        const ok = await act(`pin:${id}`, () => backend.setModelPin(id, buildId), (c: PinChange) => c.message);
        if (ok) patchState(store, { enginePins: await backend.enginePins().catch(() => store.enginePins()) });
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

      /**
       * Sets a model's parallel slots (null: the default). The engine applies them the next time the model loads;
       * a loaded model is never reloaded or interrupted, so the message says when the change takes effect.
       */
      async setSlots(id: string, slots: number | null): Promise<boolean> {
        const result: { change?: SlotsChange } = {};
        const ok = await act(`model:${id}`, async () => {
          result.change = await backend.setModelSlots(id, slots);
        });
        const c = result.change;
        if (!ok || !c) return false;
        const name = store.library().find((m) => m.id === id)?.name ?? id;
        patchState(store, { library: store.library().map((m) => (m.id === id ? { ...m, slots: c.slots, slotsCustom: slots !== null, loadedSlots: c.loadedSlots } : m)) });
        toasts.ok(c.appliesOnNextLoad ? `${slotsText(c.slots)} saved. ${name} keeps ${slotsText(c.loadedSlots ?? c.slots)} until it loads again.` : `${slotsText(c.slots)} saved.`);
        return true;
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
