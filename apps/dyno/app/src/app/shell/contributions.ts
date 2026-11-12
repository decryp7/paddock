import {
  defineCommand,
  defineDialog,
  defineHealthService,
  defineMenuItem,
  defineNotificationKind,
  definePage,
  defineSettingsPage,
  defineStatusItem,
  msg,
  type,
  type ContributionBatch,
} from '@decryp7/paddock-ui/shell/contributions';
import { aboutContributions, defineAboutDialogs } from '@decryp7/paddock-ui/shell/about';
import { defineConfirmDialog } from '@decryp7/paddock-ui/shell/confirm';
import { defineMcpKeyDialog, defineMcpRevokeDialog, defineMcpSettingsPage } from '@decryp7/paddock-ui/shell/mcp';
import { defineDiagnosticsDialog, diagnosticsContributions } from '@decryp7/paddock-ui/shell/diagnostics';
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

/** Settings > AI access (MCP) (ADR 0007, 0032 D7): off by default; each tool, and each action by its own switch, is allowed by the user. */
export const MCP_PAGE = defineMcpSettingsPage({
  providers: () => import('./mcp.providers').then((m) => m.MCP_PROVIDERS),
  load: () => import('@decryp7/paddock-ui/shell/mcp/page').then((m) => m.PdMcpPage),
});

/** The key dialog (a modal form opened from the page): the one window that issues a client key and shows it once. */
export const MCP_KEY_DIALOG = defineMcpKeyDialog({
  providers: () => import('./mcp.providers').then((m) => m.MCP_KEY_PROVIDERS),
  load: () => import('@decryp7/paddock-ui/shell/mcp/dialog').then((m) => m.PdMcpKeyDialog),
});

/** The revoke confirmation of a key (a modal decision, one window per key). */
export const MCP_REVOKE_DIALOG = defineMcpRevokeDialog({
  load: () => import('@decryp7/paddock-ui/shell/mcp/dialog').then((m) => m.PdMcpRevokeDialog),
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
  MCP_PAGE,
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

/** Report a problem (Help menu, command palette): shows every file with its size before saving; its window boots the diagnostics providers. */
export const REPORT_PROBLEM_DIALOG = defineDiagnosticsDialog({ providers: () => import('./diagnostics.providers').then((m) => m.DIAGNOSTICS_PROVIDERS) });

export const DIALOGS = [CONFIRM_DIALOG, SETTINGS_DIALOG, ALERT_RULE_DIALOG, LOCKERS_DIALOG, WEB_SEARCH_DIALOG, FILE_HASH_DIALOG, MCP_KEY_DIALOG, MCP_REVOKE_DIALOG, ...ABOUT_DIALOGS, REPORT_PROBLEM_DIALOG] as const;

/** Report a problem's command, Help menu item and the Logging settings page. */
export const DIAGNOSTICS_CONTRIBUTIONS = diagnosticsContributions([]);

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
  // Quit and stop measuring sits right above the shell's Quit in the app menu (Quit stays last: Cmd+Q) (the menu bar, and the macOS menu bar).
  menus: [defineMenuItem({ menu: 'app', command: 'dy.quit-stop', group: 'quit', order: 900 })],
  // The palette's commands beyond capture are the pages' own buttons, named as the buttons are (the category names the page).
  commands: [
    defineCommand<DynoContext>({
      id: 'dy.quit-stop',
      title: msg('dy.quit-stop', 'Quit and stop measuring'),
      category: msg('pd.category.app', 'Application'),
      run: (ctx) => ctx.quitAndStop(),
    }),
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
  healthServices: [
    defineHealthService({ id: 'dy.agent', label: msg('dy.health.agent', 'the Dyno agent'), icon: 'dy:processes' }),
    defineHealthService({ id: 'dy.tray', label: msg('dy.health.tray', 'the tray icons'), icon: 'dy:dashboard' }),
    defineHealthService({ id: 'dy.setup', label: msg('dy.health.setup', 'the installer program'), icon: 'dy:settings' }),
    defineHealthService({ id: 'dy.update', label: msg('dy.health.update', 'Dyno itself'), icon: 'dy:settings' }),
  ],
  notificationKinds: [defineNotificationKind({ id: 'dy.alarm', title: msg('dy.notification.alarm', 'Alarm'), actions: [] })],
};
