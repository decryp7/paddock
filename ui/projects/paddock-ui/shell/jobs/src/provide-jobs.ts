import { EnvironmentProviders, makeEnvironmentProviders, Provider } from '@angular/core';
import { JobFollow } from './job-follow';
import { JobsBackend } from './jobs-backend';
import { JOBS_CONFIG, JobsConfig, DEFAULT_STILL_WORKING_AFTER_MS } from './jobs-config';
import { JobsStore } from './jobs.store';

export interface ShellJobsOptions {
  /** Builds the window's backend (in an injection context): `new TauriJobsBackend(ipc)` in the app, a fake in the browser. */
  readonly backend: () => JobsBackend;
  /** After how long without an event a running job shows "Still working" (default 10 s). */
  readonly stillWorkingAfterMs?: number;
}

/**
 * The jobs of one window (rule 32, 34): the store, the follow helper and the backend. Each window that shows jobs calls it
 * in its own bootstrap; the engine is the single source of truth, so windows share jobs through the events only.
 */
export function provideShellJobs(options: ShellJobsOptions): EnvironmentProviders {
  const after = options.stillWorkingAfterMs ?? DEFAULT_STILL_WORKING_AFTER_MS;
  if (!Number.isFinite(after) || after <= 0) throw new RangeError(`stillWorkingAfterMs must be a positive number of milliseconds, not ${after}`);
  const providers: Provider[] = [
    { provide: JobsBackend, useFactory: options.backend },
    { provide: JOBS_CONFIG, useValue: Object.freeze({ stillWorkingAfterMs: after }) satisfies JobsConfig },
    JobsStore,
    JobFollow,
  ];
  return makeEnvironmentProviders(providers);
}
