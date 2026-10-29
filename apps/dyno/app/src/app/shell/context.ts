import { inject } from '@angular/core';
import type { AppContextFactory } from '@decryp7/paddock-ui/shell';
import { BACKEND } from '../core/backend';
import { Notifier } from '../shared/notifier';
import { BackupStore } from '../state/backup.store';
import { DiagnosisStore } from '../state/diagnosis.store';
import { HistoryStore } from '../state/history.store';
import { IssuesStore } from '../state/issues.store';

/** Dyno's part of the shell context: small and purpose-built (interface segregation). */
export interface DynoContext {
  /** Saves the flight recorder's last ten minutes. */
  capture(): Promise<void>;
  /** Reads the stored history of the dashboard's range again. */
  refreshHistory(): Promise<void>;
  /** Reads the OS logs now (the Issues page's button). */
  rescanIssues(): Promise<void>;
  /** Checks the backup tools now (Backup health's button). */
  checkBackup(): Promise<void>;
  /** Runs the slowness check again (Why slow's button). */
  runWhySlow(): Promise<void>;
}

/** Built once, in the root injection context, when the shell first needs the context. */
export const dynoContext: AppContextFactory<DynoContext> = () => {
  const backend = inject(BACKEND);
  const notifier = inject(Notifier);
  const history = inject(HistoryStore);
  const issues = inject(IssuesStore);
  const backup = inject(BackupStore);
  const diagnosis = inject(DiagnosisStore);
  return {
    refreshHistory: () => history.load(),
    rescanIssues: () => issues.rescan(),
    checkBackup: () => backup.check(),
    runWhySlow: () => diagnosis.run(),
    capture: async () => {
      try {
        const info = await backend.capture();
        notifier.show(`Captured the last ten minutes (${info.samples} samples)`);
      } catch (e) {
        notifier.show(e instanceof Error ? e.message : 'The capture failed.', 'bad');
      }
    },
  };
};
