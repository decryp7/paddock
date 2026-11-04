import { computed, inject } from '@angular/core';
import { patchState, signalStore, withComputed, withHooks, withMethods, withState } from '@ngrx/signals';
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

/** The waits between timed reads while the helper is not answering (ms); the last one repeats. */
export const HELPER_POLL_STEPS_MS: readonly number[] = [2000, 4000, 8000, 15_000];
/** Timed reads stop once the waits so far add up to this (ms): after about three minutes the card waits for the person. */
export const HELPER_POLL_LIMIT_MS = 180_000;

/**
 * The wait before the next timed read, or null when the state needs none (anything but "installed, not answering") or the
 * limit is used up. `waited` is the sum of the waits so far.
 */
export function nextHelperPoll(status: HelperStatus | null, step: number, waited: number): number | null {
  if (status?.state !== 'notRunning') return null;
  const wait = HELPER_POLL_STEPS_MS[Math.min(step, HELPER_POLL_STEPS_MS.length - 1)] ?? HELPER_POLL_LIMIT_MS;
  return waited + wait > HELPER_POLL_LIMIT_MS ? null : wait;
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
    // Just after Windows starts the helper's service is still coming up: while it is installed but not answering the card
    // reads again by itself, with growing waits, and stops once it answers (or is off) or the limit is used up.
    let timer: ReturnType<typeof setTimeout> | null = null;
    let step = 0;
    let waited = 0;
    function stopPolling(): void {
      if (timer !== null) clearTimeout(timer);
      timer = null;
    }
    function schedule(): void {
      stopPolling();
      if (store.action() !== null) return;
      const wait = nextHelperPoll(store.status(), step, waited);
      if (wait === null) return;
      step++;
      waited += wait;
      timer = setTimeout(() => void poll(), wait);
    }
    /** A timed read: the card stays as it is (no spinner, no error) when it fails, and the next one is scheduled. */
    async function poll(): Promise<void> {
      timer = null;
      try {
        patchState(store, { status: await backend.helperStatus() });
      } catch {
        // The last state stays on the card; the next timed read may succeed.
      }
      schedule();
    }
    async function load(): Promise<void> {
      stopPolling();
      step = 0;
      waited = 0;
      patchState(store, { loading: true, error: null });
      try {
        patchState(store, { status: await backend.helperStatus(), loading: false });
      } catch (e) {
        patchState(store, { loading: false, error: toCommandError(e) });
      }
      schedule();
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
        stopPolling();
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
      /** Stops the timed reads (the card went away). */
      stopPolling,
    };
  }),
  withHooks({
    onDestroy(store) {
      store.stopPolling();
    },
  }),
);
