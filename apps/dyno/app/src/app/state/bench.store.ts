import { DestroyRef, inject } from '@angular/core';
import { patchState, signalStore, withMethods, withState } from '@ngrx/signals';
import type { BenchComparison, BenchOverview, CommandError, StartBenchmark } from '../core/api.types';
import { BACKEND } from '../core/backend';
import { toCommandError } from '../core/command-error';

/** How often the overview is re-read while a run is in progress. */
export const BENCH_POLL_MS = 1000;

interface BenchState {
  overview: BenchOverview | null;
  comparison: BenchComparison | null;
  busy: boolean;
  error: CommandError | null;
}

/**
 * Benchmark runs. Starting is always a click; while the engine reports a run in progress the overview is polled,
 * and polling stops when it finishes or the page is left.
 */
export const BenchStore = signalStore(
  { providedIn: 'root' },
  withState<BenchState>({ overview: null, comparison: null, busy: false, error: null }),
  withMethods((store, backend = inject(BACKEND), destroyRef = inject(DestroyRef, { optional: true })) => {
    let timer: ReturnType<typeof setTimeout> | null = null;
    const stop = () => {
      if (timer !== null) clearTimeout(timer);
      timer = null;
    };
    const apply = (overview: BenchOverview) => {
      patchState(store, { overview, error: null });
      stop();
      if (overview.running) timer = setTimeout(() => void load(), BENCH_POLL_MS);
    };
    const load = async (): Promise<void> => {
      try {
        apply(await backend.benchmarks());
      } catch (e) {
        stop();
        patchState(store, { error: toCommandError(e) });
      }
    };
    destroyRef?.onDestroy(stop);
    return {
      load,
      stop,
      async start(request: StartBenchmark): Promise<void> {
        patchState(store, { busy: true, error: null, comparison: null });
        try {
          apply(await backend.startBenchmark(request));
        } catch (e) {
          patchState(store, { error: toCommandError(e) });
        } finally {
          patchState(store, { busy: false });
        }
      },
      async cancel(): Promise<void> {
        try {
          await backend.cancelBenchmark();
          await load();
        } catch (e) {
          patchState(store, { error: toCommandError(e) });
        }
      },
      async remove(id: string): Promise<void> {
        try {
          await backend.deleteBenchmark(id);
          patchState(store, { comparison: null });
          await load();
        } catch (e) {
          patchState(store, { error: toCommandError(e) });
        }
      },
      async compare(before: string, after: string): Promise<void> {
        try {
          patchState(store, { comparison: await backend.compareBenchmarks(before, after), error: null });
        } catch (e) {
          patchState(store, { error: toCommandError(e) });
        }
      },
      clearComparison(): void {
        patchState(store, { comparison: null });
      },
    };
  }),
);
