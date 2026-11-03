import { DestroyRef, inject } from '@angular/core';
import { patchState, signalStore, withMethods, withState } from '@ngrx/signals';
import type { BenchComparison, BenchOverview, CommandError, StartBenchmark } from '../core/api.types';
import { BACKEND } from '../core/backend';
import { toCommandError } from '../core/command-error';

/** How often the overview is re-read while a run is in progress. */
export const BENCH_POLL_MS = 1000;

/** A start the engine held back until the person agrees to run on battery (ADR 0030 D6): asked per run, never remembered. */
export interface BatteryConsent {
  request: StartBenchmark;
  message: string;
}

interface BenchState {
  overview: BenchOverview | null;
  comparison: BenchComparison | null;
  busy: boolean;
  error: CommandError | null;
  consent: BatteryConsent | null;
}

/**
 * Benchmark runs. Starting is always a click; while the engine reports a run in progress the overview is polled,
 * and polling stops when it finishes or the page is left.
 */
export const BenchStore = signalStore(
  { providedIn: 'root' },
  withState<BenchState>({ overview: null, comparison: null, busy: false, error: null, consent: null }),
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
        patchState(store, { busy: true, error: null, comparison: null, consent: null });
        try {
          apply(await backend.startBenchmark(request));
        } catch (e) {
          const error = toCommandError(e);
          // The engine's question is not an error: the page asks it and starts again with the answer.
          if (error.code === 'needsConsent') patchState(store, { consent: { request, message: error.message } });
          else patchState(store, { error });
        } finally {
          patchState(store, { busy: false });
        }
      },
      /** The person agreed to run on battery: start the held request again, for this run only. */
      async allowBattery(): Promise<void> {
        const held = store.consent();
        if (!held) return;
        await this.start({ ...held.request, allowBattery: true });
      },
      declineBattery(): void {
        patchState(store, { consent: null });
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
