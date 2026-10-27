import { inject } from '@angular/core';
import { patchState, signalStore, withMethods, withState } from '@ngrx/signals';
import type { CommandError, IssuesOverview, SavedIssues } from '../core/api.types';
import { BACKEND } from '../core/backend';
import { toCommandError } from '../core/command-error';
import { plural } from '../shared/format';
import { Notifier } from '../shared/notifier';
import { fileName } from './diagnostics.store';

/** The windows the page offers, in days (the engine accepts 1 to 90). */
export const ISSUE_WINDOWS = [7, 14, 30] as const;

interface IssuesState {
  days: number;
  overview: IssuesOverview | null;
  error: CommandError | null;
  /** Reading the OS logs now (can take a minute on a busy PC). */
  rescanning: boolean;
  exporting: boolean;
  saved: SavedIssues | null;
  saveError: CommandError | null;
}

/** The Issues page: what the OS recorded, for the window the person chose, and the export for Debrief. */
export const IssuesStore = signalStore(
  { providedIn: 'root' },
  withState<IssuesState>({ days: 14, overview: null, error: null, rescanning: false, exporting: false, saved: null, saveError: null }),
  withMethods((store, backend = inject(BACKEND), notifier = inject(Notifier)) => ({
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
    /** Reads the OS logs now; the page keeps showing what it has meanwhile. */
    async rescan(): Promise<void> {
      if (store.rescanning()) return;
      patchState(store, { rescanning: true });
      const days = store.days();
      try {
        const overview = await backend.rescanIssues({ days });
        patchState(store, { rescanning: false, error: null, ...(days === store.days() ? { overview } : {}) });
        notifier.show(overview.total === 0 ? 'Read the system logs: no issues in this period' : `Read the system logs: ${plural(overview.total, 'issue')} in this period`);
      } catch (e) {
        patchState(store, { rescanning: false, error: toCommandError(e) });
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
  })),
);
