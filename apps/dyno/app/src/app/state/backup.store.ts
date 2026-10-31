import { computed, inject } from '@angular/core';
import { patchState, signalStore, withComputed, withMethods, withState } from '@ngrx/signals';
import type { BackupReport, CommandError } from '../core/api.types';
import { BACKEND } from '../core/backend';
import { toCommandError } from '../core/command-error';
import { JobStarter, progressOf, type JobRun } from '../core/job-starter';
import { JobsStore } from '@decryp7/paddock-ui/shell/jobs';
import { heroText } from '../features/backup/backup-model';

/** How often the page asks again while the engine runs a check of its own (one the person did not ask for). It never gives up. */
export const BACKUP_POLL_MS = 1500;

interface BackupState {
  report: BackupReport | null;
  error: CommandError | null;
  /** The person asked for a check and the engine is running it, as a job with progress and Cancel. */
  checking: boolean;
  /** The running check's job id (its progress is the shell's job status). */
  checkJobId: number | null;
  /** Why the last "Check now" failed (the earlier report stays on screen). */
  checkError: CommandError | null;
}

/** Backup health: the last report and a check on request. The backup settings live in the Settings page. */
export const BackupStore = signalStore(
  { providedIn: 'root' },
  withState<BackupState>({ report: null, error: null, checking: false, checkJobId: null, checkError: null }),
  withComputed(({ report, checking, checkJobId }, jobs = inject(JobsStore)) => ({
    /** The running check: its phase, its count, and whether it has been quiet long enough to say "Still working". */
    checkJob: computed(() => progressOf(jobs, checkJobId())),
    statuses: computed(() => report()?.statuses ?? []),
    /** A check is running: one the person asked for, or one the engine started on its own. */
    busy: computed(() => checking() || (report()?.checking ?? false)),
    hero: computed(() => heroText(report()?.statuses ?? [])),
  })),
  withMethods((store, backend = inject(BACKEND), starter = inject(JobStarter)) => {
    let check: JobRun | null = null;
    /** Shows a report and follows the check the engine runs on its own until it says it is done. A running check is never timed out. */
    async function settle(first: BackupReport): Promise<void> {
      let report = first;
      patchState(store, { report, error: null });
      while (report.checking) {
        await new Promise<void>((resolve) => setTimeout(resolve, BACKUP_POLL_MS));
        report = await backend.backup();
        patchState(store, { report, error: null });
      }
    }
    return {
    /** Reads the last report; while the engine is checking on its own it asks again every 1.5 s until the check is done. */
    async load(): Promise<void> {
      try {
        await settle(await backend.backup());
      } catch (e) {
        patchState(store, { error: toCommandError(e) });
      }
    },
    /**
     * Checks the tools now, as a job: the engine answers at once, the page shows which tool it is on and offers Cancel. The report
     * is replaced only when every tool was read; a cancelled check leaves the earlier report as it was.
     */
    async check(): Promise<void> {
      if (store.checking()) return;
      patchState(store, { checking: true, checkJobId: null, checkError: null });
      const run = (check = starter.run('refreshBackup', (checkJobId) => patchState(store, { checkJobId })));
      try {
        const out = await run.result;
        if (out && 'backup' in out) await settle(out.backup);
      } catch (e) {
        patchState(store, { checkError: toCommandError(e) });
      } finally {
        check = null;
        patchState(store, { checking: false, checkJobId: null });
      }
    },
    /** Stops the check in progress; the earlier report stays. */
    async cancelCheck(): Promise<void> {
      try {
        await check?.cancel();
      } catch (e) {
        patchState(store, { checkError: toCommandError(e) });
      }
    },
    };
  }),
);
