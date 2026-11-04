import { computed, inject } from '@angular/core';
import { patchState, signalStore, withComputed, withMethods, withState } from '@ngrx/signals';
import { JobsStore } from '@decryp7/paddock-ui/shell/jobs';
import type { CommandError, ToolOffer } from '../core/api.types';
import { BACKEND } from '../core/backend';
import { toCommandError } from '../core/command-error';
import { JobStarter, progressOf, type JobRun } from '../core/job-starter';

interface ToolState {
  offer: ToolOffer | null;
  loading: boolean;
  error: CommandError | null;
  running: boolean;
  jobId: number | null;
  actionError: string | null;
  cancelled: boolean;
}

/**
 * The disk-health tool (smartmontools) for Settings: what would be downloaded and why the button is or is not there, and the
 * download as a job with phases and Cancel. There is no UI deadline: a quiet job is "Still working" (ER-19).
 */
export const ToolStore = signalStore(
  { providedIn: 'root' },
  withState<ToolState>({ offer: null, loading: false, error: null, running: false, jobId: null, actionError: null, cancelled: false }),
  withComputed(({ jobId }, jobs = inject(JobsStore)) => ({ job: computed(() => progressOf(jobs, jobId())) })),
  withMethods((store, backend = inject(BACKEND), starter = inject(JobStarter)) => {
    let run: JobRun | null = null;
    async function load(): Promise<void> {
      patchState(store, { loading: true, error: null });
      try {
        patchState(store, { offer: await backend.toolOffer(), loading: false });
      } catch (e) {
        patchState(store, { loading: false, error: toCommandError(e) });
      }
    }
    return {
      load,
      async ensure(): Promise<void> {
        if (store.offer() === null && !store.loading()) await load();
      },
      /** Downloads, checks and installs the tool as one job. */
      async install(): Promise<void> {
        if (store.running()) return;
        patchState(store, { running: true, jobId: null, actionError: null, cancelled: false });
        let finished = false;
        run = starter.run({ installTool: { tool: 'smartmontools' } }, (jobId) => patchState(store, { jobId }));
        try {
          const out = await run.result;
          if (out === null) patchState(store, { cancelled: true });
          else finished = true;
        } catch (e) {
          patchState(store, { actionError: toCommandError(e).message });
        } finally {
          run = null;
          patchState(store, { running: false, jobId: null });
          await load();
          // The job said done; if the card still does not see the tool, say so and why: never a silent "Not installed" (ER-27).
          const offer = store.offer();
          if (finished && store.error() === null && offer !== null && !(typeof offer.state === 'object' && 'installed' in offer.state)) {
            patchState(store, {
              actionError: `The install finished, but Dyno cannot see ${offer.name} yet (the helper did not list it). Press Check again; if it stays like this, update the helper in Settings and send the log.`,
            });
          }
        }
      },
      async cancel(): Promise<void> {
        await run?.cancel();
      },
    };
  }),
);
