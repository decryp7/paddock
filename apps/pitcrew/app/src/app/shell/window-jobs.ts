import { effect, inject, type Signal } from '@angular/core';
import type { SuJobSnapshot } from '@decryp7/paddock-ui';
import { JobsStore } from '@decryp7/paddock-ui/shell/jobs';
import { REGISTER_JOB_KIND } from '../core/job-kinds';
import { RegisterLauncher } from './register-launcher';

/** Jobs that another window of Pitcrew starts and waits for (the register window): its page says how they end. */
export const FOLLOWED_ELSEWHERE_KINDS: ReadonlySet<string> = new Set([REGISTER_JOB_KIND]);

/** The part of the jobs store this reads. */
export interface FollowableJobs {
  jobs(): readonly SuJobSnapshot[];
  follow(id: number): { stop(): void };
}

/**
 * Every window has its own jobs store, and a store announces the end of any job nobody waits for in it. A job the register window
 * starts and waits for would so be announced a second time by the main window. While that window is open this waits for such jobs
 * here too (once each, from the first snapshot), so only the wizard says how they ended; when it is gone (closed, crashed) the
 * waiting stops, and a job still running is announced here when it ends. Call it in an injection context.
 */
export function followJobsStartedElsewhere(
  store: FollowableJobs = inject(JobsStore) as unknown as FollowableJobs,
  windowOpen: Signal<boolean> = inject(RegisterLauncher).isOpen,
): void {
  const following = new Map<number, { stop(): void }>();
  effect(() => {
    if (!windowOpen()) {
      for (const f of following.values()) f.stop();
      following.clear();
      return;
    }
    for (const job of store.jobs()) {
      if (FOLLOWED_ELSEWHERE_KINDS.has(job.kind) && !following.has(job.id)) following.set(job.id, store.follow(job.id));
    }
  });
}
