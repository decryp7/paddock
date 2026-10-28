import {
  defineCommand,
  defineDialog,
  defineHealthService,
  defineKeybinding,
  defineMenuItem,
  defineNotificationKind,
  definePage,
  defineSettingsPage,
  msg,
  type,
  type ContributionBatch,
} from '@decryp7/paddock-ui/shell';
import { aboutContributions, defineAboutDialogs } from '@decryp7/paddock-ui/shell/about';
import type { SpotterContext } from './context';

// The registration file of Spotter: static data and loaders only (lazy by construction). Its lints run in
// contributions.spec.ts (`assertContributionsValid`, the manifest and the dialog contracts).

/** What the confirm dialog is asked (the opener's inputs). */
export interface ConfirmInput {
  readonly title: string;
  readonly message: string;
  readonly confirmLabel: string;
  readonly danger: boolean;
}

/** A decision: modal, fitted to its content, Cancel and one way forward. Replaces the in-page confirm dialog. */
export const CONFIRM_DIALOG = defineDialog({
  id: 'sp.confirm',
  kind: 'decision',
  title: msg('sp.dialog.confirm', 'Confirm'),
  width: 420,
  result: type<boolean>(),
  commands: [],
  load: () => import('./confirm-dialog').then((m) => m.ConfirmDialog),
});

/** Commands Spotter's settings page calls (the dialog contract names exactly these, besides the shell's settings commands). */
export const SETTINGS_PAGE_COMMANDS = [
  'app_info',
  'choose_folder',
  'get_autostart',
  'get_settings',
  'install_renderer',
  'mail_status',
  'open',
  'pause_notifications',
  'renderer_status',
  'save_settings',
  'send_test_email',
  'set_autostart',
  'set_mail_password',
] as const;

/**
 * The Settings window: the shell's window as the app's own dialog, so its contract is in this registry. A tool: modeless,
 * large, remembers its size. Spotter's page is the one below; Appearance and Updates are the shell's.
 */
export const SETTINGS_DIALOG = defineDialog({
  id: 'pd.settings',
  kind: 'tool',
  title: msg('pd.settings.title', 'Settings'),
  width: 880,
  height: 640,
  commands: ['settings_overview', 'settings_reset', 'settings_set', ...SETTINGS_PAGE_COMMANDS],
  providers: () => import('./settings-window').then((m) => m.SETTINGS_WINDOW),
  load: () => import('@decryp7/paddock-ui/shell/settings/window').then((m) => m.PdSettingsWindow),
});

/** Spotter's page of the Settings window. */
export const SETTINGS_PAGES = [
  defineSettingsPage({
    id: 'sp.general',
    group: 'application',
    title: msg('sp.settings.general', 'Spotter'),
    order: 100,
    load: () => import('../features/settings/settings-page').then((m) => m.SettingsPage),
  }),
] as const;

/** The shell's About, Updates and What's New; their windows boot Spotter's About providers. */
export const ABOUT_DIALOGS = defineAboutDialogs({ providers: () => import('./about.providers').then((m) => m.ABOUT_PROVIDERS) });

export const DIALOGS = [CONFIRM_DIALOG, SETTINGS_DIALOG, ...ABOUT_DIALOGS] as const;

/** About's commands, Help menu items and notice kind. */
export const ABOUT_CONTRIBUTIONS = aboutContributions([]);

export const CONTRIBUTIONS: ContributionBatch<SpotterContext> = {
  commands: [
    defineCommand<SpotterContext>({
      id: 'sp.check-all',
      title: msg('sp.check-all', 'Check all now'),
      category: msg('sp.category.watches', 'Watches'),
      enabled: (ctx) => ctx.hasWatches(),
      run: (ctx) => ctx.checkAll(),
    }),
    defineCommand<SpotterContext>({
      id: 'sp.mark-all-seen',
      title: msg('sp.mark-all-seen', 'Mark all as seen'),
      category: msg('sp.category.watches', 'Watches'),
      enabled: (ctx) => ctx.unseen() > 0,
      run: (ctx) => ctx.markAllSeen(),
    }),
    defineCommand<SpotterContext>({
      id: 'sp.new-watch',
      title: msg('sp.new-watch', 'Add a watch'),
      category: msg('sp.category.watches', 'Watches'),
      run: (ctx) => ctx.addWatch(),
    }),
    defineCommand<SpotterContext>({
      id: 'sp.pause-notifications',
      title: msg('sp.pause-notifications', 'Pause notifications for 1 hour'),
      category: msg('sp.category.notifications', 'Notifications'),
      run: (ctx) => ctx.pauseNotifications(60),
    }),
    defineCommand<SpotterContext>({
      id: 'sp.resume-notifications',
      title: msg('sp.resume-notifications', 'Resume notifications'),
      category: msg('sp.category.notifications', 'Notifications'),
      run: (ctx) => ctx.pauseNotifications(null),
    }),
  ],
  keybindings: [defineKeybinding({ command: 'sp.check-all', key: 'Mod+Shift+R' }), defineKeybinding({ command: 'sp.new-watch', key: 'Mod+N' })],
  menus: [
    defineMenuItem({ menu: 'file', command: 'sp.new-watch', group: 'watches', order: 100 }),
    defineMenuItem({ menu: 'file', command: 'sp.check-all', group: 'watches', order: 200 }),
    defineMenuItem({ menu: 'file', command: 'sp.mark-all-seen', group: 'watches', order: 300 }),
    defineMenuItem({ menu: 'file', command: 'sp.pause-notifications', group: 'notifications', order: 400 }),
    defineMenuItem({ menu: 'file', command: 'sp.resume-notifications', group: 'notifications', order: 500 }),
  ],
  pages: [
    definePage({ id: 'sp.dashboard', title: msg('sp.page.dashboard', 'Dashboard'), icon: 'sp:dashboard', order: 100, load: () => import('../features/dashboard/dashboard-page').then((m) => m.DashboardPage) }),
    definePage<SpotterContext>({
      id: 'sp.watches',
      title: msg('sp.page.watches', 'Watches'),
      icon: 'sp:watches',
      order: 200,
      badge: (ctx) => (ctx.unseen() > 0 ? ctx.unseen() : null),
      badgeLabel: (ctx) => (ctx.unseen() === 1 ? $localize`:@@sp.watches.unseen.one:1 release you have not seen` : $localize`:@@sp.watches.unseen.other:${ctx.unseen()}:count: releases you have not seen`),
      load: () => import('../features/watches/watches-page').then((m) => m.WatchesPage),
    }),
  ],
  settingsPages: SETTINGS_PAGES,
  dialogs: DIALOGS,
  healthServices: [
    defineHealthService({ id: 'sp.scheduler', label: msg('sp.health.scheduler', 'the watch scheduler'), icon: 'sp:history' }),
    defineHealthService({ id: 'sp.sources', label: msg('sp.health.sources', 'your sources'), icon: 'sp:watches' }),
    defineHealthService({ id: 'sp.renderer', label: msg('sp.health.renderer', 'the page browser'), icon: 'sp:settings' }),
  ],
  notificationKinds: [defineNotificationKind({ id: 'sp.release', title: msg('sp.notification.release', 'New release'), actions: [] })],
};
