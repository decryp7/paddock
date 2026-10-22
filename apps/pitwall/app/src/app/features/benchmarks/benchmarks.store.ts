import { computed, inject } from '@angular/core';
import { patchState, signalStore, withComputed, withMethods, withState } from '@ngrx/signals';
import type { BenchmarkResult, CommandError, LocalModel } from '../../core/api.types';
import { BACKEND } from '../../core/backend';
import { toCommandError } from '../../core/command-error';
import { Toasts } from '../../core/toasts';
import { rate } from '../../shared/format';

interface BenchmarksState {
  results: BenchmarkResult[];
  models: LocalModel[];
  loading: boolean;
  error: CommandError | null;
  /** Models with a run in flight. */
  running: Record<string, true>;
}

const initial: BenchmarksState = { results: [], models: [], loading: false, error: null, running: {} };

/** Benchmarks: past results on this hardware and runs per model. */
export const BenchmarksStore = signalStore(
  withState(initial),
  withComputed(({ results, models }) => ({
    sorted: computed(() => [...results()].sort((a, b) => b.at - a.at)),
    /** Models that can be benchmarked (they generate text). */
    runnable: computed(() => models().filter((m) => m.capabilities.includes('chat'))),
    names: computed(() => new Map(models().map((m) => [m.id, m.name]))),
    /** The newest result per model, for the list. */
    latest: computed(() => {
      const map = new Map<string, BenchmarkResult>();
      for (const r of results()) {
        const seen = map.get(r.modelId);
        if (!seen || r.at > seen.at) map.set(r.modelId, r);
      }
      return map;
    }),
  })),
  withMethods((store, backend = inject(BACKEND), toasts = inject(Toasts)) => ({
    async load(): Promise<void> {
      patchState(store, { loading: true, error: null });
      try {
        const [results, models] = await Promise.all([backend.benchmarks(), backend.localModels()]);
        patchState(store, { results, models, loading: false });
      } catch (e) {
        patchState(store, { loading: false, error: toCommandError(e) });
      }
    },

    isRunning(modelId: string): boolean {
      return modelId in store.running();
    },

    /** Whether any run is in flight (Run all waits for it). */
    anyRunning(): boolean {
      return Object.keys(store.running()).length > 0;
    },

    /** Runs every chat model one after the other (each run loads its model, so never in parallel); a run in flight is skipped. */
    async runAll(): Promise<void> {
      for (const m of store.runnable()) await this.run(m.id);
    },

    async run(modelId: string): Promise<void> {
      if (modelId in store.running()) return;
      patchState(store, { running: { ...store.running(), [modelId]: true } });
      try {
        const result = await backend.runBenchmark(modelId);
        patchState(store, { results: [result, ...store.results()] });
        toasts.ok(`${store.names().get(modelId) ?? modelId}: ${rate(result.genTps)} tok/s.`);
      } catch (e) {
        toasts.error(toCommandError(e));
      } finally {
        const running = { ...store.running() };
        delete running[modelId];
        patchState(store, { running });
      }
    },
  })),
);

export type BenchmarksStore = InstanceType<typeof BenchmarksStore>;
