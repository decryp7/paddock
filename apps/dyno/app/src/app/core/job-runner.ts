import { Injectable, inject } from '@angular/core';
import type { SuJobSnapshot } from '@decryp7/paddock-ui';
import type { CommandError, JobOutput, JobSpec, JobStatus } from './api.types';
import { BACKEND } from './backend';
import { toCommandError } from './command-error';

/** How often a running job's status is read. A job that is running is never given up on: there is no timeout here. */
export const JOB_POLL_MS = 500;
/** No change in a running job for this long shows "Still working" (never an error). */
export const JOB_STALLED_MS = 8_000;

/** A job as a page shows it: the status in the shared job component's shape, and whether it has stood still for a while. */
export interface JobProgress {
  snapshot: SuJobSnapshot;
  stalled: boolean;
}

/** The status of the engine's job in the shape of `<pd-job>` (the flat state tag the shared component reads). */
export function toSnapshot(s: JobStatus): SuJobSnapshot {
  const base = {
    id: s.id,
    kind: s.kind,
    title: s.title,
    subject: s.subject,
    seq: s.seq,
    elapsedMs: s.elapsedMs,
    etaMs: s.etaMs,
    cancellable: s.cancellable,
    notCancellableReason: s.notCancellableReason,
  };
  const st = s.state;
  if (st === 'queued') return { ...base, state: 'queued' };
  if (st === 'done') return { ...base, state: 'done' };
  if (st === 'cancelled') return { ...base, state: 'cancelled', reason: 'user' };
  if ('failed' in st) return { ...base, state: 'failed', message: st.failed.message };
  return { ...base, state: 'running', phase: st.running.phase };
}

/** A job that was started: stop it, and wait for what it produced. */
export interface JobRun {
  /** Asks the engine to stop the job and resolves at once; `result` resolves `null` when it has stopped. */
  cancel(): Promise<void>;
  /** The output of a done job; `null` when the job was cancelled; rejects with the engine's message when it failed. */
  readonly result: Promise<JobOutput | null>;
}

/**
 * Runs the engine's long operations (ER-17 to ER-19): starts the job, which answers at once, then reads its status until it
 * ends and passes each change to `onProgress`. It never times a job out; a job that stands still is reported `stalled`, which
 * the page shows as "Still working". Only the failure to reach the engine ends a run early.
 */
@Injectable({ providedIn: 'root' })
export class JobRunner {
  private readonly backend = inject(BACKEND);

  run(spec: JobSpec, onProgress: (progress: JobProgress) => void): JobRun {
    let timer: ReturnType<typeof setTimeout> | undefined;
    let lastSeq = -1;
    let lastChange = Date.now();
    const started = this.backend.startJob(spec);
    const publish = (s: JobStatus) => {
      if (s.seq !== lastSeq) {
        lastSeq = s.seq;
        lastChange = Date.now();
      }
      const stalled = Date.now() - lastChange >= JOB_STALLED_MS && typeof s.state === 'object' && 'running' in s.state;
      onProgress({ snapshot: toSnapshot(s), stalled });
    };
    const result = new Promise<JobOutput | null>((resolve, reject) => {
      const fail = (e: unknown) => reject(toCommandError(e));
      const settle = async (s: JobStatus): Promise<void> => {
        if (s.state === 'done') return resolve(await this.backend.jobResult(s.id));
        if (s.state === 'cancelled') return resolve(null);
        if (typeof s.state === 'object' && 'failed' in s.state) return reject({ code: 'internal', message: s.state.failed.message } satisfies CommandError);
      };
      const poll = async (id: number): Promise<void> => {
        try {
          const mine = (await this.backend.jobs()).find((j) => j.id === id);
          if (!mine) return reject({ code: 'internal', message: 'Dyno lost track of this job. Try again.' } satisfies CommandError);
          publish(mine);
          if (mine.state === 'done' || mine.state === 'cancelled' || (typeof mine.state === 'object' && 'failed' in mine.state)) return await settle(mine);
          timer = setTimeout(() => void poll(id), JOB_POLL_MS);
        } catch (e) {
          fail(e);
        }
      };
      started.then((first) => {
        publish(first);
        return poll(first.id);
      }, fail);
    });
    // Whatever the outcome, no poll is left behind.
    const done = () => clearTimeout(timer);
    result.then(done, done);
    return {
      cancel: async () => {
        const job = await started;
        await this.backend.cancelJob(job.id);
      },
      result,
    };
  }
}
