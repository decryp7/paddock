import { Injector, inject } from '@angular/core';
import { patchState, signalStore, withHooks, withMethods, withState } from '@ngrx/signals';
import { JobsStore } from '@decryp7/paddock-ui/shell/jobs';
import type { CommandError, Neighbour, ThisPcWol, WolDeviceInput, WolOutcome, WolView } from '../core/api.types';
import { BACKEND, type Unsubscribe } from '../core/backend';
import { toCommandError } from '../core/command-error';
import { JobStarter, progressOf, type JobProgress, type JobRun } from '../core/job-starter';

/** One wake of one computer: its job, and what it ended with. */
export interface WakeRun {
  /** The job's id once the engine accepted it (its progress is the shell's job status). */
  jobId: number | null;
  /** A test wake waits for an answer. */
  test: boolean;
  /** `null` while it runs or when it was cancelled or failed. */
  outcome: WolOutcome | null;
  /** Why it failed or was refused (a message from the engine). */
  error: CommandError | null;
  /** Still running. */
  running: boolean;
  /** Cancel was pressed. */
  cancelled: boolean;
}

interface WolState {
  view: WolView | null;
  error: CommandError | null;
  /** A save or remove is on its way to the agent. */
  busy: boolean;
  /** Why the last save or remove failed (the list stays as it was). */
  actionError: CommandError | null;
  /** "Add from network": the entries not yet saved; `null` until asked for. */
  candidates: Neighbour[] | null;
  candidatesLoading: boolean;
  candidatesError: CommandError | null;
  readiness: ThisPcWol | null;
  readinessLoading: boolean;
  readinessError: CommandError | null;
  /** The latest wake of each computer, by id. */
  runs: Record<number, WakeRun>;
}

/**
 * Wake devices: the agent's saved computers, "Add from network", this PC's readiness, and the wakes. The agent decides and
 * validates; a wake is a job (phases, Cancel, "Still working") followed with the shell's own mechanism.
 */
export const WolStore = signalStore(
  { providedIn: 'root' },
  withState<WolState>({
    view: null,
    error: null,
    busy: false,
    actionError: null,
    candidates: null,
    candidatesLoading: false,
    candidatesError: null,
    readiness: null,
    readinessLoading: false,
    readinessError: null,
    runs: {},
  }),
  withMethods((store, backend = inject(BACKEND), starter = inject(JobStarter), injector = inject(Injector)) => {
    const live = new Map<number, JobRun>();
    function setRun(id: number, patch: Partial<WakeRun>): void {
      const prev = store.runs()[id] ?? { jobId: null, test: false, outcome: null, error: null, running: false, cancelled: false };
      patchState(store, { runs: { ...store.runs(), [id]: { ...prev, ...patch } } });
    }
    async function act(call: () => Promise<WolView>): Promise<boolean> {
      patchState(store, { busy: true, actionError: null });
      try {
        patchState(store, { view: await call(), error: null });
        return true;
      } catch (e) {
        patchState(store, { actionError: toCommandError(e) });
        return false;
      } finally {
        patchState(store, { busy: false });
      }
    }
    return {
      async load(): Promise<void> {
        try {
          patchState(store, { view: await backend.wolDevices(), error: null });
        } catch (e) {
          patchState(store, { error: toCommandError(e) });
        }
      },
      save: (device: WolDeviceInput) => act(() => backend.wolSave(device)),
      remove: (id: number) => act(() => backend.wolRemove(id)),
      clearActionError(): void {
        patchState(store, { actionError: null });
      },
      /** Reads the neighbour table (on demand: the page asks when the person opens "Add from network"). */
      async loadCandidates(): Promise<void> {
        patchState(store, { candidatesLoading: true, candidatesError: null });
        try {
          patchState(store, { candidates: await backend.wolCandidates() });
        } catch (e) {
          patchState(store, { candidatesError: toCommandError(e) });
        } finally {
          patchState(store, { candidatesLoading: false });
        }
      },
      async loadReadiness(): Promise<void> {
        patchState(store, { readinessLoading: true, readinessError: null });
        try {
          patchState(store, { readiness: await backend.wolReadiness() });
        } catch (e) {
          patchState(store, { readinessError: toCommandError(e) });
        } finally {
          patchState(store, { readinessLoading: false });
        }
      },
      /** Wakes a computer as a job; `test` also waits for it to answer and says whether it woke. */
      async wake(id: number, test: boolean): Promise<void> {
        if (store.runs()[id]?.running) return;
        setRun(id, { jobId: null, test, outcome: null, error: null, running: true, cancelled: false });
        const run = starter.run({ wolWake: { id, test } }, (jobId) => setRun(id, { jobId }));
        live.set(id, run);
        try {
          const out = await run.result;
          if (out && 'wol' in out) setRun(id, { outcome: out.wol });
          else setRun(id, { cancelled: true });
        } catch (e) {
          setRun(id, { error: toCommandError(e) });
        } finally {
          live.delete(id);
          setRun(id, { running: false });
        }
      },
      /** Stops a wake in progress. */
      async cancel(id: number): Promise<void> {
        try {
          await live.get(id)?.cancel();
        } catch (e) {
          setRun(id, { error: toCommandError(e) });
        }
      },
      /** The running wake's phase and count, and whether it has been quiet long enough to say "Still working". */
      progress(id: number): JobProgress | null {
        // The shell's jobs store is asked for when a wake runs: a window that never wakes anything has none.
        return progressOf(injector.get(JobsStore), store.runs()[id]?.jobId ?? null);
      },
      receive(view: WolView): void {
        patchState(store, { view, error: null });
      },
    };
  }),
  withHooks((store) => {
    const backend = inject(BACKEND);
    let stop: Unsubscribe | null = null;
    return {
      onInit() {
        stop = backend.onWol((v) => store.receive(v));
      },
      onDestroy() {
        stop?.();
      },
    };
  }),
);
