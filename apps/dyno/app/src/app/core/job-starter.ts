import { DestroyRef, Injectable, Injector, inject } from '@angular/core';
import type { SuJobSnapshot } from '@decryp7/paddock-ui';
import { JobFollow, JobsStore, type JobsStoreType } from '@decryp7/paddock-ui/shell/jobs';
import type { CommandError, JobOutput, JobSpec } from './api.types';
import { BACKEND } from './backend';

/** A job as a page shows it: the shell's status of it, and whether it has been quiet long enough to say "Still working". */
export interface JobProgress {
  snapshot: SuJobSnapshot;
  stalled: boolean;
}

/** The progress of job `id` from the shell's jobs store (`null` while it has no id or the store has not heard of it yet). */
export function progressOf(jobs: JobsStoreType, id: number | null): JobProgress | null {
  if (id === null) return null;
  const snapshot = jobs.jobs().find((j) => j.id === id);
  return snapshot ? { snapshot, stalled: jobs.stalledIds().has(id) } : null;
}

/** A job that was started: stop it, and wait for what it produced. */
export interface JobRun {
  /** Asks the engine to stop the job (the shell's `job_cancel`); resolves at once, false when it was already over. */
  cancel(): Promise<boolean>;
  /** The output of a done job; `null` when it was cancelled; rejects with the engine's message when it failed. */
  readonly result: Promise<JobOutput | null>;
}

/**
 * Starts one of the engine's long operations (ER-17 to ER-19) and follows it with the shell's own mechanism: the start answers
 * at once with the job's id, `JobsStore` keeps its status from the `paddock://jobs` events (so the shell's Jobs view shows it
 * too), `JobFollow` waits for its end, and Cancel is the shell's. Nothing here polls or times out: a quiet job is "Still
 * working" (the store's threshold), and a job that failed rejects with the engine's message.
 */
@Injectable({ providedIn: 'root' })
export class JobStarter {
  private readonly backend = inject(BACKEND);
  /** The shell's follow helper and jobs store are asked for when a job starts: a window that never starts one (the web search dialog) has neither. */
  private readonly injector = inject(Injector);
  private readonly owner = inject(DestroyRef);

  /** Starts `spec`; `started` hears the job's id as soon as the engine has accepted it. */
  run(spec: JobSpec, started?: (id: number) => void): JobRun {
    const follow = this.injector.get(JobFollow);
    const jobs = this.injector.get(JobsStore);
    const accepted = this.backend.startJob(spec);
    // A refusal reaches `result`; `cancel` before an accepted start has nothing to stop.
    const result = (async (): Promise<JobOutput | null> => {
      const end = await follow.run(() => accepted, this.owner, started);
      if (!end) return null;
      switch (end.state) {
        case 'done':
          return this.backend.jobResult(end.id);
        case 'failed':
          throw { code: 'internal', message: end.message } satisfies CommandError;
        default:
          return null;
      }
    })();
    return {
      result,
      cancel: async () => jobs.cancel(await accepted),
    };
  }
}
