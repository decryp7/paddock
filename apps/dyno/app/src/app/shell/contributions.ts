import {
  defineCommand,
  defineDialog,
  defineHealthService,
  defineNotificationKind,
  definePage,
  defineSettingsPage,
  defineStatusItem,
  msg,
  type,
  type ContributionBatch,
} from '@decryp7/paddock-ui/shell';
import { aboutContributions, defineAboutDialogs } from '@decryp7/paddock-ui/shell/about';
import { defineConfirmDialog } from '@decryp7/paddock-ui/shell/confirm';
import { DIALOG_CONTRACTS } from '../generated/dialog-contracts';
import type { DynoContext } from './context';
import { NAV } from './nav';

// The registration file of Dyno: static data and loaders only (lazy by construction). Its lints run in
// contributions.spec.ts (`assertContributionsValid`, the manifest and the dialog contracts).

/**
 * The Settings window: the shell's window as the app's own dialog, since its pages are the app's choice (the contract is
 * in `src-tauri/src/dialogs.rs`). A tool: modeless, large, remembers its size. Dyno's page is the one below; Appearance and
 * Updates are the shell's.
 */
export const SETTINGS_DIALOG = defineDialog(DIALOG_CONTRACTS['pd.settings'], {
  title: msg('pd.settings.title', 'Settings'),
  providers: () => import('./settings-window').then((m) => m.SETTINGS_WINDOW),
  load: () => import('@decryp7/paddock-ui/shell/settings/window').then((m) => m.PdSettingsWindow),
});

/** Dyno's page of the Settings window. */
export const SETTINGS_PAGES = [
  defineSettingsPage({
    id: 'dy.general',
    group: 'application',
    title: msg('dy.settings.general', 'Dyno'),
    order: 100,
    load: () => import('../features/settings/settings-page').then((m) => m.SettingsPage),
  }),
] as const;

/** "Add or edit an alert rule" (modal form). Answers true once the rule is saved. */
export const ALERT_RULE_DIALOG = defineDialog(DIALOG_CONTRACTS['dy.alert-rule'], {
  title: msg('dy.alert-rule.title', 'Alert rule'),
  result: type<true>(),
  load: () => import('../features/alarms/rule-dialog').then((m) => m.RuleDialog),
});

/** "What is locking this file?" (a read-only tool). */
export const LOCKERS_DIALOG = defineDialog(DIALOG_CONTRACTS['dy.lockers'], {
  title: msg('dy.lockers.title', 'What is locking this file?'),
  load: () => import('../features/processes/process-dialogs').then((m) => m.LockersDialog),
});

/** "Search the web" about a right-clicked text (modal form). Answers true once the browser was opened. */
export const WEB_SEARCH_DIALOG = defineDialog(DIALOG_CONTRACTS['dy.web-search'], {
  title: msg('dy.web-search.title', 'Search the web'),
  result: type<true>(),
  load: () => import('../features/lookup/web-search-dialog').then((m) => m.WebSearchDialog),
});

/** A program's SHA-256 with a button that opens VirusTotal's page for it (viewer). */
export const FILE_HASH_DIALOG = defineDialog(DIALOG_CONTRACTS['dy.file-hash'], {
  title: msg('dy.file-hash.title', "Search the file's hash on VirusTotal"),
  result: type<true>(),
  load: () => import('../features/lookup/hash-search-dialog').then((m) => m.HashSearchDialog),
});

/** The shell's About, Updates and What's New; their windows boot Dyno's About providers. */
export const ABOUT_DIALOGS = defineAboutDialogs({ providers: () => import('./about.providers').then((m) => m.ABOUT_PROVIDERS) });

/** The shared confirmation (ADR 0026 §5): deleting an alert rule and forgetting the known programs ask with it. */
export const CONFIRM_DIALOG = defineConfirmDialog({
  load: () => import('@decryp7/paddock-ui/shell/confirm/dialog').then((m) => m.PdConfirmDialog),
});

export const DIALOGS = [CONFIRM_DIALOG, SETTINGS_DIALOG, ALERT_RULE_DIALOG, LOCKERS_DIALOG, WEB_SEARCH_DIALOG, FILE_HASH_DIALOG, ...ABOUT_DIALOGS] as const;

/** About's commands, Help menu items and notice kind. */
export const ABOUT_CONTRIBUTIONS = aboutContributions([]);

/** Every screen of the sidebar, in order; the screens load their code on first visit. */
const PAGES = NAV.map((item, index) =>
  definePage({
    id: `dy.${item.path === '/' ? 'dashboard' : item.path.slice(1)}`,
    title: msg(`dy.page.${item.path === '/' ? 'dashboard' : item.path.slice(1)}`, item.label),
    icon: `dy:${item.icon}`,
    order: (index + 1) * 100,
    load: item.load,
  }),
);

export const CONTRIBUTIONS: ContributionBatch<DynoContext> = {
  // The palette's commands beyond capture are the pages' own buttons, named as the buttons are (the category names the page).
  commands: [
    defineCommand<DynoContext>({
      id: 'dy.capture',
      title: msg('dy.capture', 'Capture the last ten minutes'),
      category: msg('dy.category.recorder', 'Flight recorder'),
      run: (ctx) => ctx.capture(),
    }),
    defineCommand<DynoContext>({
      id: 'dy.refresh-history',
      title: msg('dy.refresh-history', 'Refresh history'),
      category: msg('dy.category.dashboard', 'Dashboard'),
      enabled: (ctx) => ctx.canRefreshHistory(),
      run: (ctx) => ctx.refreshHistory(),
    }),
    defineCommand<DynoContext>({
      id: 'dy.rescan-issues',
      title: msg('dy.rescan-issues', 'Read the system logs now'),
      category: msg('dy.category.issues', 'Issues'),
      enabled: (ctx) => ctx.canRescanIssues(),
      run: (ctx) => ctx.rescanIssues(),
    }),
    defineCommand<DynoContext>({
      id: 'dy.export-issues',
      title: msg('dy.export-issues', 'Export for Debrief…'),
      category: msg('dy.category.issues', 'Issues'),
      enabled: (ctx) => ctx.canExportIssues(),
      run: (ctx) => ctx.exportIssues(),
    }),
    defineCommand<DynoContext>({
      id: 'dy.check-backup',
      title: msg('dy.check-backup', 'Check now'),
      category: msg('dy.category.backup', 'Backup health'),
      enabled: (ctx) => ctx.canCheckBackup(),
      run: (ctx) => ctx.checkBackup(),
    }),
    defineCommand<DynoContext>({
      id: 'dy.run-why-slow',
      title: msg('dy.run-why-slow', 'Run again'),
      category: msg('dy.category.why-slow', 'Why slow'),
      enabled: (ctx) => ctx.canRunWhySlow(),
      run: (ctx) => ctx.runWhySlow(),
    }),
    defineCommand<DynoContext>({
      id: 'dy.export-report',
      title: msg('dy.export-report', 'Export Markdown…'),
      category: msg('dy.category.reports', 'Reports'),
      enabled: (ctx) => ctx.canExportReport(),
      run: (ctx) => ctx.exportReport(),
    }),
    defineCommand<DynoContext>({
      id: 'dy.save-diagnostics',
      title: msg('dy.save-diagnostics', 'Save bundle…'),
      category: msg('dy.category.diagnostics', 'Diagnostics'),
      enabled: (ctx) => ctx.canSaveDiagnostics(),
      run: (ctx) => ctx.saveDiagnostics(),
    }),
    defineCommand<DynoContext>({
      id: 'dy.add-alarm-rule',
      title: msg('dy.add-alarm-rule', 'Add rule…'),
      category: msg('dy.category.alarms', 'Alarms'),
      run: (ctx) => ctx.addAlarmRule(),
    }),
  ],
  pages: PAGES,
  statusItems: [
    defineStatusItem<DynoContext>({
      id: 'dy.agent',
      title: msg('dy.status.agent', 'The Dyno agent'),
      side: 'end',
      priority: 700,
      load: () => import('./agent-status').then((m) => m.AgentStatusItem),
    }),
  ],
  settingsPages: SETTINGS_PAGES,
  dialogs: DIALOGS,
  healthServices: [defineHealthService({ id: 'dy.agent', label: msg('dy.health.agent', 'the Dyno agent'), icon: 'dy:processes' })],
  notificationKinds: [defineNotificationKind({ id: 'dy.alarm', title: msg('dy.notification.alarm', 'Alarm'), actions: [] })],
};
