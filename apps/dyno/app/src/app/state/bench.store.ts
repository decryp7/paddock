import { DestroyRef, inject } from '@angular/core';
import { patchState, signalStore, withMethods, withState } from '@ngrx/signals';
import type { BenchEstimateRequest, BenchRunEstimate, BenchExportFormat, BenchComparison, BenchKind, BenchOverview, CommandError, GpuAdapter, SavedReport, StartBenchmark, Volume } from '../core/api.types';
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
  /** The volumes the disk test can be pointed at (the disk map's list). */
  volumes: Volume[];
  /** The last request, kept so a drive that needs a folder can be run again with one. */
  last: StartBenchmark | null;
  /** The engine's comparison of the newest run with the one before it ("compared with your last run"). */
  lastComparison: BenchComparison | null;
  /** The graphics adapters for the picker. */
  adapters: GpuAdapter[];
  /** The engine's estimate of what is chosen; null while a drive is still to be chosen. */
  estimate: BenchRunEstimate | null;
  /** Where the last export went (shown as a confirmation). */
  exported: SavedReport | null;
}

/**
 * Benchmark runs. Starting is always a click; while the engine reports a run in progress the overview is polled,
 * and polling stops when it finishes or the page is left.
 */
export const BenchStore = signalStore(
  { providedIn: 'root' },
  withState<BenchState>({ overview: null, comparison: null, busy: false, error: null, consent: null, volumes: [], last: null, lastComparison: null, adapters: [], estimate: null, exported: null }),
  withMethods((store, backend = inject(BACKEND), destroyRef = inject(DestroyRef, { optional: true })) => {
    let timer: ReturnType<typeof setTimeout> | null = null;
    const stop = () => {
      if (timer !== null) clearTimeout(timer);
      timer = null;
    };
    const compareLast = async (overview: BenchOverview): Promise<void> => {
      const [latest, before] = overview.runs;
      if (!latest || !before || overview.running) {
        patchState(store, { lastComparison: null });
        return;
      }
      try {
        // The engine decides what is comparable (same method, same drive); a refusal is simply no comparison.
        patchState(store, { lastComparison: await backend.compareBenchmarks(before.id, latest.id) });
      } catch {
        patchState(store, { lastComparison: null });
      }
    };
    const apply = (overview: BenchOverview) => {
      patchState(store, { overview, error: null });
      void compareLast(overview);
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
        patchState(store, { busy: true, error: null, comparison: null, consent: null, last: request });
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
      /** Reads the drives for the picker; a failure leaves the list empty and the system drive is used. */
      async loadVolumes(): Promise<void> {
        try {
          patchState(store, { volumes: await backend.diskVolumes() });
        } catch {
          patchState(store, { volumes: [] });
        }
      },
      /** A drive whose top is not writable: the person chooses a folder on it (the OS dialog) and that drive runs again. */
      async runWithFolder(volumeId: string): Promise<void> {
        const base = store.last();
        const folder = await backend.pickFolder();
        if (!base || !folder) return;
        await this.start({ ...base, kinds: ['disk'], diskVolumes: [volumeId], diskFolders: [{ volume: volumeId, folder }], label: `${base.label} (${folder.split(/[\\/]/).pop() ?? 'folder'})` });
      },
      /** Runs one kind again with the request of the last run (Retry on a result that failed). */
      async retryKind(kind: BenchKind): Promise<void> {
        const base = store.last();
        if (!base) return;
        await this.start({ ...base, kinds: [kind], label: `${base.label} (retry)` });
      },
      /** Asks the engine what the chosen run takes and writes; a failure leaves no estimate. */
      async estimateFor(request: BenchEstimateRequest): Promise<void> {
        try {
          patchState(store, { estimate: await backend.benchEstimate(request) });
        } catch {
          patchState(store, { estimate: null });
        }
      },
      async loadAdapters(): Promise<void> {
        try {
          patchState(store, { adapters: await backend.benchGpuAdapters() });
        } catch {
          patchState(store, { adapters: [] });
        }
      },
      /** The OS save dialog, then the engine writes the runs; cancelling the dialog changes nothing. */
      async exportRuns(ids: string[], format: BenchExportFormat): Promise<void> {
        try {
          const saved = await backend.saveBenchmarks(ids, format);
          if (saved) patchState(store, { exported: saved, error: null });
        } catch (e) {
          patchState(store, { error: toCommandError(e) });
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
