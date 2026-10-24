import {
  defineCommand,
  defineDialog,
  defineHealthService,
  defineKeybinding,
  defineMenuItem,
  definePage,
  defineSettingsPage,
  defineStatusItem,
  msg,
  type,
  type ContributionBatch,
} from '@decryp7/paddock-ui/shell';
import { aboutContributions, defineAboutDialogs } from '@decryp7/paddock-ui/shell/about';
import type { Routes } from '@angular/router';
import { PAGE_DATA } from '@decryp7/paddock-ui/shell/pages';
import { CONFIG_DIFF_DIALOG_ID } from '../features/runners/config/config-diff-dialog';
import { STOP_PROCESS_DIALOG_ID } from '../features/runners/stop-process-dialog';
import { CONFIRM_DIALOG_ID } from '../shared/ui/confirm-dialog';
import type { PitcrewContext } from './pitcrew-context';
import { REGISTER_DIALOG_ID } from './register-launcher';

// The registration file of Pitcrew on the shell: static data and loaders only. Its lints run in `pitcrew.contributions.spec.ts`
// (contributions, dialog contracts and the manifest the Rust side checks).

/** The service's id in Rust (`health::SERVICE`). */
export const SERVICE_ID = 'pc.service';

/** A confirmation of something that cannot be undone: modal, fitted to its content. */
export const CONFIRM_DIALOG = defineDialog({
  id: CONFIRM_DIALOG_ID,
  kind: 'decision',
  title: msg('pc.dialog.confirm', 'Please confirm'),
  width: 440,
  result: type<boolean>(),
  commands: [],
  load: () => import('../shared/ui/confirm-dialog').then((m) => m.ConfirmDialog),
});

/** Stopping one process of a runner's tree. */
export const STOP_PROCESS_DIALOG = defineDialog({
  id: STOP_PROCESS_DIALOG_ID,
  kind: 'decision',
  title: msg('pc.dialog.stop-process', 'Stop process'),
  width: 460,
  result: type<boolean>(),
  commands: [],
  load: () => import('../features/runners/stop-process-dialog').then((m) => m.StopProcessDialog),
});

/** What saving a runner's configuration would change: a modal form with Cancel and Save and apply. */
export const CONFIG_DIFF_DIALOG = defineDialog({
  id: CONFIG_DIFF_DIALOG_ID,
  kind: 'form',
  modality: 'modal',
  title: msg('pc.dialog.config-diff', 'Review configuration changes'),
  width: 720,
  height: 560,
  result: type<boolean>(),
  commands: [],
  load: () => import('../features/runners/config/config-diff-dialog').then((m) => m.ConfigDiffDialog),
});

/** The registration wizard: a modal form; it registers the runner (a job) and needs the status. */
export const REGISTER_DIALOG = defineDialog({
  id: REGISTER_DIALOG_ID,
  kind: 'form',
  modality: 'modal',
  title: msg('pc.dialog.register', 'Register a runner'),
  width: 660,
  height: 640,
  result: type<boolean>(),
  commands: ['register_runner', 'runner_status', 'connection_state', 'platform_name'],
  load: () => import('../features/register/register-wizard').then((m) => m.RegisterWizard),
});

/** The commands Pitcrew's settings pages call (the Settings window is a dialog, so its contract names them). */
export const SETTINGS_COMMANDS = [
  'dialog_open',
  'settings_overview',
  'settings_reset',
  'settings_set',
  'get_settings',
  'save_settings',
  'runner_status',
  'connection_state',
  'platform_name',
  'get_autostart',
  'set_autostart',
  'service_install_state',
  'install_service',
  'uninstall_service',
  'repair_service',
  'open_login_items',
  'channels_get',
  'channels_save',
  'channel_secret_set',
  'channel_secret_clear',
  'channel_secret_present',
  'channel_test',
  'has_api_token',
  'set_api_token',
  'clear_api_token',
  'export_diagnostics',
  'open_url',
] as const;

/** The Settings window (the shell's) as the app's own dialog, so its contract is in this registry. */
export const SETTINGS_DIALOG = defineDialog({
  id: 'pd.settings',
  kind: 'tool',
  title: msg('pd.settings.title', 'Settings'),
  width: 920,
  height: 680,
  commands: SETTINGS_COMMANDS,
  providers: () => import('./settings-window').then((m) => m.PITCREW_SETTINGS_WINDOW),
  load: () => import('@decryp7/paddock-ui/shell/settings/window').then((m) => m.PdSettingsWindow),
});

/** Pitcrew's settings pages (Appearance is the shell's). Each is a component of its own; Rust has no section for them. */
export const PITCREW_SETTINGS_PAGES = [
  defineSettingsPage({
    id: 'pc.general',
    group: 'application',
    title: msg('pc.settings.general', 'General'),
    order: 100,
    load: () => import('../features/settings/general-section').then((m) => m.GeneralSection),
  }),
  defineSettingsPage({
    id: 'pc.service',
    group: 'application',
    title: msg('pc.settings.service', 'Service'),
    order: 200,
    load: () => import('../features/settings/service-section').then((m) => m.ServiceSection),
  }),
  defineSettingsPage({
    id: 'pc.defaults',
    group: 'features',
    title: msg('pc.settings.defaults', 'Defaults'),
    order: 100,
    load: () => import('../features/settings/defaults-section').then((m) => m.DefaultsSection),
  }),
  defineSettingsPage({
    id: 'pc.channels',
    group: 'features',
    title: msg('pc.settings.channels', 'Notification channels'),
    order: 200,
    load: () => import('../features/settings/channels/channels-section').then((m) => m.ChannelsSection),
  }),
  defineSettingsPage({
    id: 'pc.tokens',
    group: 'features',
    title: msg('pc.settings.tokens', 'Gitea tokens'),
    order: 300,
    load: () => import('../features/settings/tokens-section').then((m) => m.TokensSection),
  }),
  defineSettingsPage({
    id: 'pc.diagnostics',
    group: 'advanced',
    title: msg('pc.settings.diagnostics', 'Diagnostics'),
    order: 100,
    load: () => import('../features/settings/diagnostics-section').then((m) => m.DiagnosticsSection),
  }),
] as const;

/** About, Updates and What's New (the shell's); their windows boot Pitcrew's About providers. */
export const ABOUT_DIALOGS = defineAboutDialogs({ providers: () => import('./about.providers').then((m) => m.ABOUT_PROVIDERS) });

/** Every dialog window of Pitcrew. */
export const PITCREW_DIALOGS = [CONFIRM_DIALOG, STOP_PROCESS_DIALOG, CONFIG_DIFF_DIALOG, REGISTER_DIALOG, SETTINGS_DIALOG, ...ABOUT_DIALOGS] as const;

/** About's commands, Help menu items and notice kind (its dialogs are among `PITCREW_DIALOGS`). */
export const ABOUT_CONTRIBUTIONS = aboutContributions([]);

export const PITCREW_CONTRIBUTIONS: ContributionBatch<PitcrewContext> = {
  commands: [
    defineCommand<PitcrewContext>({
      id: 'pc.register',
      title: msg('pc.register', 'Register a runner…'),
      category: msg('pc.category.runners', 'Runners'),
      run: async (ctx) => void (await ctx.register()),
    }),
    defineCommand<PitcrewContext>({
      id: 'pc.diagnostics',
      title: msg('pc.diagnostics', 'Export diagnostics'),
      category: msg('pc.category.runners', 'Runners'),
      run: (ctx) => ctx.exportDiagnostics(),
    }),
  ],
  keybindings: [defineKeybinding({ command: 'pc.register', key: 'Mod+Shift+N' })],
  menus: [
    defineMenuItem({ menu: 'file', command: 'pc.register', group: 'runners', order: 100 }),
    defineMenuItem({ menu: 'file', command: 'pc.diagnostics', group: 'runners', order: 200 }),
  ],
  pages: [
    definePage({ id: 'pc.dashboard', title: msg('pc.page.dashboard', 'Dashboard'), icon: 'pc:dashboard', order: 100, load: () => import('../features/dashboard/dashboard-page').then((m) => m.DashboardPage) }),
    definePage<PitcrewContext>({
      id: 'pc.runners',
      title: msg('pc.page.runners', 'Runners'),
      icon: 'pc:runner',
      order: 200,
      badge: (ctx) => ctx.attention() || null,
      badgeLabel: (ctx) => (ctx.attention() === 1 ? $localize`:@@pc.runners.attention.one:1 runner needs attention` : $localize`:@@pc.runners.attention.other:${ctx.attention()}:count: runners need attention`),
      load: () => import('../features/runners/runners-page').then((m) => m.RunnersPage),
    }),
    definePage({ id: 'pc.activity', title: msg('pc.page.activity', 'Activity'), icon: 'pc:activity', order: 300, load: () => import('../features/activity/activity-page').then((m) => m.ActivityPage) }),
    definePage({ id: 'pc.insights', title: msg('pc.page.insights', 'Insights'), icon: 'pc:insights', order: 400, load: () => import('../features/insights/insights-page').then((m) => m.InsightsPage) }),
    definePage({ id: 'pc.storage', title: msg('pc.page.storage', 'Storage'), icon: 'pc:storage', order: 500, load: () => import('../features/storage/storage-page').then((m) => m.StoragePage) }),
    definePage({ id: 'pc.logs', title: msg('pc.page.logs', 'Logs'), icon: 'pc:logs', order: 600, load: () => import('../features/logs/logs-page').then((m) => m.LogsPage) }),
    definePage<PitcrewContext>({
      id: 'pc.updates',
      title: msg('pc.page.updates', 'Updates'),
      icon: 'pc:updates',
      order: 700,
      badge: (ctx) => ctx.updatesAvailable() || null,
      badgeLabel: (ctx) => (ctx.updatesAvailable() === 1 ? $localize`:@@pc.updates.available.one:1 update available` : $localize`:@@pc.updates.available.other:${ctx.updatesAvailable()}:count: updates available`),
      load: () => import('../features/updates/updates-page').then((m) => m.UpdatesPage),
    }),
  ],
  statusItems: [
    defineStatusItem({ id: 'pc.runners', title: msg('pc.status.runners', 'Runners'), side: 'start', priority: 50, load: () => import('./runners-status').then((m) => m.RunnersStatus) }),
  ],
  settingsPages: PITCREW_SETTINGS_PAGES,
  dialogs: PITCREW_DIALOGS,
  healthServices: [defineHealthService({ id: SERVICE_ID, label: msg('pc.health.service', 'the Pitcrew service'), icon: 'pd:engine' })],
};

/** Routes besides the pages: one runner and one runner's log (matched before the pages; they belong to Runners and Logs). */
export const PITCREW_ROUTES: Routes = [
  { path: 'runners/:name', title: 'Runner', data: { [PAGE_DATA]: 'pc.runners' }, loadComponent: () => import('./pages').then((m) => m.RunnerRoute) },
  { path: 'logs/:runner', title: 'Logs', data: { [PAGE_DATA]: 'pc.logs' }, loadComponent: () => import('./pages').then((m) => m.LogsRoute) },
];
