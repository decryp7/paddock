import { inject } from '@angular/core';
import type { AppContextFactory } from '@decryp7/paddock-ui/shell';
import { DialogService } from '@decryp7/paddock-ui/shell/dialogs';
import { ShellPages } from '@decryp7/paddock-ui/shell/pages';
import { BACKEND } from '../core/backend';
import { Notifier } from '../shared/notifier';
import { BackupStore } from '../state/backup.store';
import { DiagnosisStore } from '../state/diagnosis.store';
import { DiagnosticsStore } from '../state/diagnostics.store';
import { HistoryStore } from '../state/history.store';
import { IssuesStore } from '../state/issues.store';
import { ReportsStore } from '../state/reports.store';
import { SettingsStore } from '../state/settings.store';

/**
 * Dyno's part of the shell context: small and purpose-built (interface segregation). Each action of a page that the
 * command palette offers shows that page first (so the result, a busy state or an error is where the person looks, and
 * the focus lands there), then does what the page's button does. `can*` mirror the buttons' disabled states.
 */
export interface DynoContext {
  /** Saves the flight recorder's last ten minutes. */
  capture(): Promise<void>;
  /** Dashboard: reads the stored history of the shown range again. */
  refreshHistory(): Promise<void>;
  canRefreshHistory(): boolean;
  /** Issues: reads the OS logs now. */
  rescanIssues(): Promise<void>;
  canRescanIssues(): boolean;
  /** Issues: saves the window for Debrief. */
  exportIssues(): Promise<void>;
  canExportIssues(): boolean;
  /** Backup health: checks the backup tools now. */
  checkBackup(): Promise<void>;
  canCheckBackup(): boolean;
  /** Why slow: runs the check again. */
  runWhySlow(): Promise<void>;
  canRunWhySlow(): boolean;
  /** Reports: saves the shown week as Markdown. */
  exportReport(): Promise<void>;
  canExportReport(): boolean;
  /** Diagnostics: saves the bundle. */
  saveDiagnostics(): Promise<void>;
  canSaveDiagnostics(): boolean;
  /** Alarms: opens the new-rule window (reads the settings first when they are not loaded yet). */
  addAlarmRule(): Promise<void>;
}

const ALERT_RULE_DIALOG_ID = 'dy.alert-rule';

/** Built once, in the root injection context, when the shell first needs the context. */
export const dynoContext: AppContextFactory<DynoContext> = () => {
  const backend = inject(BACKEND);
  const notifier = inject(Notifier);
  const pages = inject(ShellPages);
  const dialogs = inject(DialogService);
  const history = inject(HistoryStore);
  const issues = inject(IssuesStore);
  const backup = inject(BackupStore);
  const diagnosis = inject(DiagnosisStore);
  const reports = inject(ReportsStore);
  const diagnostics = inject(DiagnosticsStore);
  const settings = inject(SettingsStore);

  return {
    capture: async () => {
      try {
        const info = await backend.capture();
        notifier.show(`Captured the last ten minutes (${info.samples} samples)`);
      } catch (e) {
        notifier.show(e instanceof Error ? e.message : 'The capture failed.', 'bad');
      }
    },
    refreshHistory: async () => {
      await pages.goAndFocus('dy.dashboard');
      await history.load();
    },
    canRefreshHistory: () => !history.loading(),
    rescanIssues: async () => {
      await pages.goAndFocus('dy.issues');
      await issues.rescan();
    },
    canRescanIssues: () => !issues.rescanning(),
    exportIssues: async () => {
      await pages.goAndFocus('dy.issues');
      if (!issues.overview()) await issues.load();
      await issues.exportForDebrief();
    },
    canExportIssues: () => !issues.exporting(),
    checkBackup: async () => {
      await pages.goAndFocus('dy.backup');
      // The page's button waits for the first report; so does the command.
      if (!backup.report()) await backup.load();
      await backup.check();
    },
    canCheckBackup: () => !backup.busy(),
    runWhySlow: async () => {
      await pages.goAndFocus('dy.why-slow');
      await diagnosis.run();
    },
    canRunWhySlow: () => !diagnosis.running(),
    exportReport: async () => {
      await pages.goAndFocus('dy.reports');
      if (!reports.report()) await reports.load();
      await reports.export('markdown');
    },
    canExportReport: () => reports.exporting() === null,
    saveDiagnostics: async () => {
      await pages.goAndFocus('dy.diagnostics');
      await diagnostics.save();
    },
    canSaveDiagnostics: () => !diagnostics.saving(),
    addAlarmRule: async () => {
      await pages.goAndFocus('dy.alarms');
      if (!settings.settings()) await settings.load();
      const current = settings.settings();
      if (!current) return;
      const outcome = await dialogs.open<true>(ALERT_RULE_DIALOG_ID, {
        inputs: { rule: null, takenIds: (current.customAlerts ?? []).map((r) => r.id) },
        title: 'Add alert rule',
      }).closed;
      if (outcome.kind === 'answered') await settings.load();
    },
  };
};
