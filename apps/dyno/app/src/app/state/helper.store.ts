import { computed, inject } from '@angular/core';
import { patchState, signalStore, withComputed, withMethods, withState } from '@ngrx/signals';
import { JobsStore } from '@decryp7/paddock-ui/shell/jobs';
import type { CommandError, HelperAction, HelperStatus } from '../core/api.types';
import { BACKEND } from '../core/backend';
import { toCommandError } from '../core/command-error';
import { JobStarter, progressOf, type JobRun } from '../core/job-starter';

interface HelperState {
  /** The helper as the engine reports it; null until first read. */
  status: HelperStatus | null;
  loading: boolean;
  /** Reading the state failed (the engine could not be asked at all); Retry reads again. */
  error: CommandError | null;
  /** The running action and its job: the card shows the job's phase and Cancel. */
  action: HelperAction | null;
  jobId: number | null;
  /** Why the last action did not finish (declined, failed, did not answer), until the next one starts. */
  actionError: string | null;
  /** The last action was cancelled before it finished. */
  cancelled: boolean;
}

/**
 * The privileged helper for Settings (ADR 0027): its state, and Turn on / Repair / Update / Turn off as one job with phases and
 * Cancel. The job has no UI deadline: a quiet job is "Still working" (ER-19). Absent, silent and foreign helpers are states
 * of `status`, never errors.
 */
export const HelperStore = signalStore(
  { providedIn: 'root' },
  withState<HelperState>({ status: null, loading: false, error: null, action: null, jobId: null, actionError: null, cancelled: false }),
  withComputed(({ jobId }, jobs = inject(JobsStore)) => ({
    /** The running action's phase, count and whether it has been quiet long enough to say "Still working". */
    job: computed(() => progressOf(jobs, jobId())),
  })),
  withMethods((store, backend = inject(BACKEND), starter = inject(JobStarter)) => {
    let run: JobRun | null = null;
    async function load(): Promise<void> {
      patchState(store, { loading: true, error: null });
      try {
        patchState(store, { status: await backend.helperStatus(), loading: false });
      } catch (e) {
        patchState(store, { loading: false, error: toCommandError(e) });
      }
    }
    return {
      load,
      /** Reads the state the first time the card shows. */
      async ensure(): Promise<void> {
        if (store.status() === null && !store.loading()) await load();
      },
      /** Runs `action` as a job; the state is the job's output when it finished. */
      async start(action: HelperAction): Promise<void> {
        if (store.action() !== null) return;
        patchState(store, { action, jobId: null, actionError: null, cancelled: false });
        run = starter.run({ helper: { action } }, (jobId) => patchState(store, { jobId }));
        try {
          const out = await run.result;
          if (out === null) patchState(store, { cancelled: true });
          else if ('helper' in out) patchState(store, { status: out.helper });
        } catch (e) {
          patchState(store, { actionError: toCommandError(e).message });
        } finally {
          run = null;
          patchState(store, { action: null, jobId: null });
          // Whatever happened, the state on screen is the engine's, not a guess.
          await load();
        }
      },
      /** Stops the running action; it takes effect when the permission prompt has been answered. */
      async cancel(): Promise<void> {
        await run?.cancel();
      },
    };
  }),
);
