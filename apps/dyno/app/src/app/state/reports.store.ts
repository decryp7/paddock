import { inject } from '@angular/core';
import { patchState, signalStore, withMethods, withState } from '@ngrx/signals';
import type { CommandError, ReportFormat, SavedReport, WeeklyReport } from '../core/api.types';
import { BACKEND } from '../core/backend';
import { toCommandError } from '../core/command-error';
import { Notifier } from '../shared/notifier';
import { fileName } from '../shared/format';

/** Dyno keeps about a year of history: the engine refuses weeks further back. */
export const MAX_WEEKS_AGO = 52;

interface ReportsState {
  /** Which week is shown: 0 = the last seven days, 1 = the seven days before them, and so on. */
  weeksAgo: number;
  report: WeeklyReport | null;
  /** Where the newest week ends (the report of week 0), for labelling the older weeks. */
  latestEnd: number | null;
  loading: boolean;
  error: CommandError | null;
  exporting: ReportFormat | null;
  saved: SavedReport | null;
  saveError: CommandError | null;
}

/** The weekly report: one week at a time from the engine, and its export to a file the person picks. */
export const ReportsStore = signalStore(
  { providedIn: 'root' },
  withState<ReportsState>({ weeksAgo: 0, report: null, latestEnd: null, loading: false, error: null, exporting: null, saved: null, saveError: null }),
  withMethods((store, backend = inject(BACKEND), notifier = inject(Notifier)) => ({
    async load(): Promise<void> {
      const weeksAgo = store.weeksAgo();
      patchState(store, { loading: true, error: null });
      try {
        const report = await backend.weeklyReport(weeksAgo);
        // A slower answer for a week the person already left must not replace the one they chose.
        if (weeksAgo !== store.weeksAgo()) return;
        patchState(store, (s) => ({ report, loading: false, latestEnd: weeksAgo === 0 ? report.to : s.latestEnd }));
      } catch (e) {
        if (weeksAgo === store.weeksAgo()) patchState(store, { loading: false, error: toCommandError(e) });
      }
    },
    async select(weeksAgo: number): Promise<void> {
      const week = Math.min(Math.max(0, Math.trunc(weeksAgo)), MAX_WEEKS_AGO);
      if (week === store.weeksAgo() && store.report()) return;
      patchState(store, { weeksAgo: week, report: null, saved: null, saveError: null });
      await this.load();
    },
    /** Shows the OS save dialog through the shell and writes the report there; a cancelled dialog changes nothing. */
    async export(format: ReportFormat): Promise<void> {
      if (store.exporting()) return;
      patchState(store, { exporting: format, saveError: null });
      try {
        const saved = await backend.saveReport(format, store.weeksAgo());
        patchState(store, { exporting: null });
        if (saved) {
          patchState(store, { saved });
          notifier.show(`Saved ${fileName(saved.path)}`);
        }
      } catch (e) {
        patchState(store, { exporting: null, saveError: toCommandError(e) });
      }
    },
  })),
);
