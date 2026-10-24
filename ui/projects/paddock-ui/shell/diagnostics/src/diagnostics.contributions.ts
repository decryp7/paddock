import { defineCommand, defineDialog, defineMenuItem, defineSettingsPage, msg, type, type ContributionBatch, type DialogDef, type ProvidersLoader } from '@decryp7/paddock-ui/shell';
import type { Saved } from '../generated/diagnostics';
import type { DiagnosticsCommand } from '../generated/diagnostics-commands';

// The diagnostics part's contributions: static data and loaders only (lazy by construction, plan §9). The dialog's component
// and the providers its window needs are chunks of their own.

/** The dialog id (stable: routes, remembered bounds, test ids). */
export const DIAGNOSTICS_DIALOG_ID = 'pd.report-problem';

/**
 * What the Report a Problem window may call (its contract; Rust's guard allows exactly these): the three report commands
 * and the webview's log command, so the window's own errors reach the log too.
 */
export const DIAGNOSTICS_DIALOG_COMMANDS = ['diagnostics_preview', 'diagnostics_save', 'diagnostics_cancel', 'log_from_webview'] as const satisfies readonly DiagnosticsCommand[];

/** The id of the "Logging" settings page; the Rust section of the same id is `paddock_diag::logging::LOGGING`. */
export const LOGGING_SETTINGS_PAGE = 'pd.logging';

export interface DiagnosticsDialogOptions {
  /**
   * The providers the dialog window boots with: the app's `provideShellDiagnostics` with its backend (`TauriDiagnosticsBackend`
   * over its IPC), from a module of its own so they stay lazy.
   */
  readonly providers: ProvidersLoader;
}

/** The Report a Problem dialog (ADR 0009): a decision, modal, fitted to its content; it resolves with the saved report or nothing. */
export function defineDiagnosticsDialog(options: DiagnosticsDialogOptions): DialogDef {
  return defineDialog({
    id: DIAGNOSTICS_DIALOG_ID,
    kind: 'decision',
    title: msg('pd.diag.title', 'Report a problem'),
    width: 680,
    result: type<Saved>(),
    commands: DIAGNOSTICS_DIALOG_COMMANDS,
    providers: options.providers,
    load: () => import('./report-problem-dialog').then((m) => m.PdReportProblemDialog),
  });
}

/**
 * The command and Help menu item that open the dialog, and the Logging settings page (the Rust section of the same id is
 * registered by `Builder::diagnostics`). Add it to the main window's contributions with `defineDiagnosticsDialog`'s dialog
 * among the app's dialogs.
 */
export function diagnosticsContributions(dialogs: readonly DialogDef[]): ContributionBatch {
  return {
    commands: [
      defineCommand({
        id: 'pd.diagnostics.report',
        title: msg('pd.diag.open', 'Report a problem…'),
        category: msg('pd.category.help', 'Help'),
        run: (ctx) => void ctx.openDialog(DIAGNOSTICS_DIALOG_ID),
      }),
    ],
    menus: [defineMenuItem({ menu: 'help', command: 'pd.diagnostics.report', group: 'support', order: 400 })],
    settingsPages: [defineSettingsPage({ id: LOGGING_SETTINGS_PAGE, group: 'advanced', title: msg('pd.settings.logging', 'Logging'), order: 900 })],
    dialogs,
  };
}
