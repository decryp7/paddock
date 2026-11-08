import { computed, inject } from '@angular/core';
import { patchState, signalStore, withComputed, withMethods, withState } from '@ngrx/signals';
import type { CommandError, IssuesOverview, SavedIssues } from '../core/api.types';
import { BACKEND } from '../core/backend';
import { toCommandError } from '../core/command-error';
import { JobStarter, progressOf, type JobRun } from '../core/job-starter';
import { JobsStore } from '@decryp7/paddock-ui/shell/jobs';
import { fileName, plural } from '../shared/format';
import { Notifier } from '../shared/notifier';

/** The windows the page offers, in days (the engine accepts 1 to 90). */
export const ISSUE_WINDOWS = [7, 14, 30] as const;

interface IssuesState {
  days: number;
  overview: IssuesOverview | null;
  error: CommandError | null;
  /** Reading the OS logs now (can take a minute on a busy PC); it runs as a job with progress and Cancel. */
  rescanning: boolean;
  /** The running read's job id (its progress is the shell's job status). */
  rescanJobId: number | null;
  exporting: boolean;
  saved: SavedIssues | null;
  saveError: CommandError | null;
}

/** The Issues page: what the OS recorded, for the window the person chose, and the export for Debrief. */
export const IssuesStore = signalStore(
  { providedIn: 'root' },
  withState<IssuesState>({ days: 14, overview: null, error: null, rescanning: false, rescanJobId: null, exporting: false, saved: null, saveError: null }),
  withComputed(({ rescanJobId }, jobs = inject(JobsStore)) => ({
    /** The running read: its phase, its count, and whether it has been quiet long enough to say "Still working". */
    rescanJob: computed(() => progressOf(jobs, rescanJobId())),
  })),
  withMethods((store, backend = inject(BACKEND), notifier = inject(Notifier), starter = inject(JobStarter)) => {
    let rescan: JobRun | null = null;
    return {
    async load(): Promise<void> {
      const days = store.days();
      try {
        const overview = await backend.issues({ days });
        // A slower answer for a window the person already left must not replace the one they chose.
        if (days === store.days()) patchState(store, { overview, error: null });
      } catch (e) {
        if (days === store.days()) patchState(store, { error: toCommandError(e) });
      }
    },
    async setDays(days: number): Promise<void> {
      if (days === store.days()) return;
      patchState(store, { days });
      await this.load();
    },
    /**
     * Reads the OS logs now, as a job: the page keeps showing what it has, shows the read's progress and offers Cancel.
     * A cancelled read changes nothing.
     */
    async rescan(): Promise<void> {
      if (store.rescanning()) return;
      patchState(store, { rescanning: true, rescanJobId: null });
      const days = store.days();
      const run = (rescan = starter.run({ rescanIssues: { query: { days } } }, (rescanJobId) => patchState(store, { rescanJobId })));
      try {
        const out = await run.result;
        if (out === null) {
          patchState(store, { error: null });
          notifier.show('Stopped reading the system logs');
        } else if ('issues' in out) {
          const overview = out.issues;
          patchState(store, { error: null, ...(days === store.days() ? { overview } : {}) });
          notifier.show(overview.total === 0 ? 'Read the system logs: no issues in this period' : `Read the system logs: ${plural(overview.total, 'issue')} in this period`);
        }
      } catch (e) {
        patchState(store, { error: toCommandError(e) });
      } finally {
        rescan = null;
        patchState(store, { rescanning: false, rescanJobId: null });
      }
    },
    /** Stops the read in progress; the job ends as cancelled and the page keeps what it had. */
    async cancelRescan(): Promise<void> {
      try {
        await rescan?.cancel();
      } catch (e) {
        patchState(store, { error: toCommandError(e) });
      }
    },
    /** Shows the OS save dialog through the shell and writes the window as JSON lines for Debrief; a cancelled dialog changes nothing. */
    async exportForDebrief(): Promise<void> {
      if (store.exporting()) return;
      patchState(store, { exporting: true, saveError: null });
      try {
        const saved = await backend.saveIssues(store.days());
        patchState(store, { exporting: false });
        if (saved) {
          patchState(store, { saved });
          notifier.show(`Saved ${fileName(saved.path)} (${plural(saved.count, 'issue')})`);
        }
      } catch (e) {
        patchState(store, { exporting: false, saveError: toCommandError(e) });
      }
    },
    };
  }),
);
