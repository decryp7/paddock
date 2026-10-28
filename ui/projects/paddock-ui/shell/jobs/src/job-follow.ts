import { DestroyRef, inject, Injectable } from '@angular/core';
import type { SuJobSnapshot } from '@decryp7/paddock-ui';
import { JobsStore } from './jobs.store';

/**
 * Work a dialog or page starts as a job: follow it while the owner lives. Closing the owner only stops following; the job
 * goes on and its end is announced as a notice. Nothing here times out (ADR 0008 §4): a quiet job shows "Still working".
 *
 * This is how an app waits for a job: never with its own `paddock://jobs` listener (the shell's notice would be doubled;
 * conformance C40), and without a second success toast for the same job.
 */
@Injectable()
export class JobFollow {
  private readonly store = inject(JobsStore);

  /** The job's terminal snapshot, or null when `owner` was destroyed first (or already was). */
  follow(id: number, owner: DestroyRef): Promise<SuJobSnapshot | null> {
    const following = this.store.follow(id);
    return new Promise((resolve) => {
      let off: () => void;
      try {
        off = owner.onDestroy(() => {
          following.stop();
          resolve(null);
        });
      } catch {
        following.stop();
        resolve(null);
        return;
      }
      void following.finished.then((job) => {
        off();
        resolve(job);
      });
    });
  }

  /**
   * Starts the work with `start` (an app command that registers the job and resolves with its id at once), then follows
   * it while `owner` lives. Rejects only when the work did not start.
   */
  async run(start: () => Promise<number>, owner: DestroyRef, started?: (id: number) => void): Promise<SuJobSnapshot | null> {
    const id = await start();
    started?.(id);
    return this.follow(id, owner);
  }
}
