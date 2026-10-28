import { effect, inject } from '@angular/core';
import type { SuJobSnapshot } from '@decryp7/paddock-ui';
import { JobsStore } from '@decryp7/paddock-ui/shell/jobs';

/** Jobs that another window of Pitcrew starts and waits for (the register window): its page says how they end. */
export const FOLLOWED_ELSEWHERE_KINDS: ReadonlySet<string> = new Set(['pitcrew.register-runner']);

/** The part of the jobs store this reads. */
export interface FollowableJobs {
  jobs(): readonly SuJobSnapshot[];
  follow(id: number): unknown;
}

/**
 * Every window has its own jobs store, and a store announces the end of any job nobody waits for in it. A job the register window
 * starts and waits for would so be announced a second time by the main window. This waits for such jobs here too (once each,
 * from the first snapshot), so only the page that started them says how they ended. Call it in an injection context.
 */
export function followJobsStartedElsewhere(store: FollowableJobs = inject(JobsStore) as unknown as FollowableJobs): void {
  const followed = new Set<number>();
  effect(() => {
    for (const job of store.jobs()) {
      if (FOLLOWED_ELSEWHERE_KINDS.has(job.kind) && !followed.has(job.id)) {
        followed.add(job.id);
        store.follow(job.id);
      }
    }
  });
}
