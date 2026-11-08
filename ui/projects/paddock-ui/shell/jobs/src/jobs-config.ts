import { InjectionToken } from '@angular/core';

/** After this long without an event, a running job shows "Still working" (never an error, never a timeout). */
export const DEFAULT_STILL_WORKING_AFTER_MS = 10_000;

/** The shell's jobs settings for a window (`provideShellJobs`). */
export interface JobsConfig {
  /** How long a running job may be silent before it shows "Still working". */
  readonly stillWorkingAfterMs: number;
}

export const JOBS_CONFIG = new InjectionToken<JobsConfig>('PADDOCK_JOBS_CONFIG', {
  providedIn: 'root',
  factory: () => ({ stillWorkingAfterMs: DEFAULT_STILL_WORKING_AFTER_MS }),
});

/**
 * The clock of the jobs store: the time, and a repeating tick that refreshes "Still working" while a job runs. A port so
 * tests drive it by hand. The tick only re-reads the time; it never ends, fails or abandons a job.
 */
export interface JobsClock {
  now(): number;
  /** Calls `tick` every `ms` until the returned function is called. */
  every(ms: number, tick: () => void): () => void;
  /** Calls `fire` once after `ms` unless the returned function is called first. */
  after(ms: number, fire: () => void): () => void;
}

export const JOBS_CLOCK = new InjectionToken<JobsClock>('PADDOCK_JOBS_CLOCK', {
  providedIn: 'root',
  factory: () => ({
    now: () => Date.now(),
    every: (ms, tick) => {
      const handle = setInterval(tick, ms);
      return () => clearInterval(handle);
    },
    after: (ms, fire) => {
      const handle = setTimeout(fire, ms);
      return () => clearTimeout(handle);
    },
  }),
});
