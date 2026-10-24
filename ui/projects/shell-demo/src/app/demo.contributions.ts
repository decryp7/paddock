import {
  defineCommand,
  defineDialog,
  defineHealthService,
  defineKeybinding,
  defineMenuItem,
  defineNotificationKind,
  definePage,
  defineSettingsPage,
  defineStatusItem,
  msg,
  type,
  type ContributionBatch,
} from '@decryp7/paddock-ui/shell';
import { defineSubmenu, type SubmenuDef } from '@decryp7/paddock-ui/shell/menu';
import { aboutContributions, defineAboutDialogs } from '@decryp7/paddock-ui/shell/about';
import { defineDiagnosticsDialog, diagnosticsContributions } from '@decryp7/paddock-ui/shell/diagnostics';
import { defineLanguageSettingsPage } from '@decryp7/paddock-ui/shell/i18n/settings';
import { defineServiceSettingsPage } from '@decryp7/paddock-ui/shell/service';
import type { DemoContext } from './demo-context';
import type { DemoStatus } from './dialogs/demo-ipc';

// The registration file of the shell demo: static data and loaders only (plan §9). Its lints run in
// shell/testing/src/reference-apps.spec.ts (contributions) and e2e/shell-demo.spec.ts (lazy imports).

/** A decision: modal, fitted to its content. */
export const CONFIRM_RESET_DIALOG = defineDialog({
  id: 'sd.confirm-reset',
  kind: 'decision',
  title: msg('sd.dialog.confirm-reset', 'Reset greetings?'),
  width: 420,
  result: type<boolean>(),
  commands: [],
  load: () => import('./dialogs/confirm-reset-dialog').then((m) => m.ConfirmResetDialog),
});

/** A form that blocks its opener. */
export const RENAME_DIALOG = defineDialog({
  id: 'sd.rename',
  kind: 'form',
  modality: 'modal',
  title: msg('sd.dialog.rename', 'Rename'),
  width: 440,
  result: type<string>(),
  commands: [],
  load: () => import('./dialogs/rename-dialog').then((m) => m.RenameDialog),
});

/** A viewer: modeless, 680x560 at least, grows only. */
export const ABOUT_DIALOG = defineDialog({
  id: 'sd.about',
  kind: 'viewer',
  title: msg('sd.dialog.about', 'About the shell demo'),
  commands: [],
  load: () => import('./dialogs/about-dialog').then((m) => m.AboutDialog),
});

/** A tool: modeless, large, remembers its size; it calls the engine, so its contract names that command. */
export const INSPECTOR_DIALOG = defineDialog({
  id: 'sd.inspector',
  kind: 'tool',
  title: msg('sd.dialog.inspector', 'Engine inspector'),
  width: 720,
  height: 480,
  result: type<DemoStatus>(),
  commands: ['demo_status'],
  providers: () => import('./dialogs/demo-ipc').then((m) => m.INSPECTOR_PROVIDERS),
  load: () => import('./dialogs/inspector-dialog').then((m) => m.InspectorDialog),
});

/** A required choice: a modal without Cancel. */
export const CHOOSE_STYLE_DIALOG = defineDialog({
  id: 'sd.choose-style',
  kind: 'decision',
  title: msg('sd.dialog.choose-style', 'Choose a greeting style'),
  width: 400,
  requiredChoice: true,
  result: type<'casual' | 'formal'>(),
  commands: [],
  load: () => import('./dialogs/choose-style-dialog').then((m) => m.ChooseStyleDialog),
});

/**
 * The Settings window (plan S10): the shell's window as the app's own dialog, so its contract is in this registry. A tool:
 * modeless, large, remembers its size; it reads and changes settings through the settings commands only.
 */
export const SETTINGS_DIALOG = defineDialog({
  id: 'pd.settings',
  kind: 'tool',
  title: msg('pd.settings.title', 'Settings'),
  width: 880,
  height: 620,
  // The settings commands, and the Background service page's (it asks and repairs the service from this window).
  commands: ['settings_overview', 'settings_reset', 'settings_set', 'service_status', 'service_repair', 'service_install', 'open_login_items'],
  providers: () => import('./demo-settings-window').then((m) => m.DEMO_SETTINGS_WINDOW),
  load: () => import('@decryp7/paddock-ui/shell/settings/window').then((m) => m.PdSettingsWindow),
});

/** Settings > Background service (plan X4): status, Install, Repair, Open Login Items and "Start at login"; its providers load with the page. */
export const SERVICE_PAGE = defineServiceSettingsPage({
  providers: () => import('./service.providers').then((m) => m.SERVICE_PROVIDERS),
  load: () => import('@decryp7/paddock-ui/shell/service/page').then((m) => m.PdServicePage),
});

/** Settings > Language: System default, English and the pseudo-locale (the demo ships no real translation). */
export const LANGUAGE_PAGE = defineLanguageSettingsPage({
  load: () => import('@decryp7/paddock-ui/shell/i18n/page').then((m) => m.PdLanguagePage),
});

/** The demo's settings pages: those without a component of their own show the Rust section of the same id in the shell's generic form. */
export const DEMO_SETTINGS_PAGES = [
  LANGUAGE_PAGE,
  SERVICE_PAGE,
  defineSettingsPage({ id: 'sd.general', group: 'application', title: msg('sd.settings.general', 'General'), order: 100 }),
  defineSettingsPage({ id: 'sd.storage', group: 'features', title: msg('sd.settings.storage', 'Storage'), order: 100 }),
] as const;

/** The shell's About, Updates and What's New (plan S11); their windows boot the demo's About providers. */
export const SHELL_ABOUT_DIALOGS = defineAboutDialogs({ providers: () => import('./about.providers').then((m) => m.ABOUT_PROVIDERS) });

/** Report a problem (plan X2): a decision that shows every file with its size before saving; its window boots the demo's diagnostics providers. */
export const REPORT_PROBLEM_DIALOG = defineDiagnosticsDialog({ providers: () => import('./diagnostics.providers').then((m) => m.DIAGNOSTICS_PROVIDERS) });

export const DEMO_DIALOGS = [CONFIRM_RESET_DIALOG, RENAME_DIALOG, ABOUT_DIALOG, INSPECTOR_DIALOG, CHOOSE_STYLE_DIALOG, SETTINGS_DIALOG, ...SHELL_ABOUT_DIALOGS, REPORT_PROBLEM_DIALOG] as const;

/** About's commands, Help menu items and notice kind (its dialogs are among `DEMO_DIALOGS`). */
export const SHELL_ABOUT_CONTRIBUTIONS = aboutContributions([]);

/** Report a problem's command, Help menu item and Logging settings page (its dialog is among `DEMO_DIALOGS`). */
export const SHELL_DIAGNOSTICS_CONTRIBUTIONS = diagnosticsContributions([]);

/** The titles of the theme choices (literal msg() calls: the catalog scanner reads them). */
const THEME_TITLES = {
  system: msg('sd.theme.system', 'Theme: system'),
  dark: msg('sd.theme.dark', 'Theme: dark'),
  light: msg('sd.theme.light', 'Theme: light'),
} as const;

export const DEMO_CONTRIBUTIONS: ContributionBatch<DemoContext> = {
  commands: [
    defineCommand<DemoContext>({
      id: 'sd.greet',
      title: msg('sd.greet', 'Say hello'),
      category: msg('sd.category.demo', 'Demo'),
      run: (ctx) => {
        const n = ctx.greet();
        ctx.notify({ level: 'info', kind: 'sd.greeting', message: `Hello from the shell demo (${n})` });
      },
    }),
    defineCommand<DemoContext>({
      id: 'sd.reset',
      title: msg('sd.reset', 'Reset greetings'),
      category: msg('sd.category.demo', 'Demo'),
      enabled: (ctx) => ctx.greetings() > 0,
      run: (ctx) => {
        ctx.reset();
        ctx.notify({ level: 'success', message: 'Greetings reset' });
      },
    }),
    defineCommand<DemoContext>({
      id: 'sd.job.start',
      title: msg('sd.job.start', 'Start sample job'),
      category: msg('sd.category.jobs', 'Jobs'),
      run: async (ctx) => void (await ctx.startJob()),
    }),
    defineCommand<DemoContext>({
      id: 'sd.job.quiet',
      title: msg('sd.job.quiet', 'Start quiet job'),
      category: msg('sd.category.jobs', 'Jobs'),
      run: async (ctx) => void (await ctx.startQuietJob()),
    }),
    defineCommand<DemoContext>({
      id: 'sd.compact',
      title: msg('sd.compact', 'Compact layout'),
      category: msg('sd.category.demo', 'Demo'),
      checked: (ctx) => ctx.compact(),
      run: (ctx) => ctx.toggleCompact(),
    }),
    defineCommand<DemoContext>({
      id: 'sd.notify.three',
      title: msg('sd.notify.three', 'Notify three times'),
      category: msg('sd.category.notify', 'Notifications'),
      run: (ctx) => {
        for (let i = 0; i < 3; i++) ctx.notify({ level: 'warning', kind: 'sd.greeting', message: 'The disk is almost full' });
      },
    }),
    defineCommand<DemoContext>({
      id: 'sd.engine.break',
      title: msg('sd.engine.break', 'Break the demo engine'),
      category: msg('sd.category.notify', 'Notifications'),
      run: (ctx) => ctx.breakEngine(),
    }),
    defineCommand<DemoContext>({
      id: 'sd.index.break',
      title: msg('sd.index.break', 'Break the search index'),
      category: msg('sd.category.notify', 'Notifications'),
      run: (ctx) => ctx.breakIndex(),
    }),
    ...(['system', 'dark', 'light'] as const).map((theme) =>
      defineCommand<DemoContext>({
        id: `sd.theme.${theme}`,
        title: THEME_TITLES[theme],
        category: msg('sd.category.appearance', 'Appearance'),
        checked: (ctx) => ctx.theme() === theme,
        run: (ctx) => ctx.setTheme(theme),
      }),
    ),
    defineCommand<DemoContext>({
      id: 'sd.warm-paper',
      title: msg('sd.warm-paper', 'Warm paper'),
      category: msg('sd.category.appearance', 'Appearance'),
      checked: (ctx) => ctx.pack() === 'sd.theme.warm-paper',
      run: (ctx) => ctx.setPack(ctx.pack() ? null : 'sd.theme.warm-paper'),
    }),
    defineCommand<DemoContext>({
      id: 'sd.contrast',
      title: msg('sd.contrast', 'High contrast'),
      category: msg('sd.category.appearance', 'Appearance'),
      checked: (ctx) => ctx.highContrast(),
      run: (ctx) => ctx.toggleHighContrast(),
    }),
    defineCommand<DemoContext>({
      id: 'sd.motion',
      title: msg('sd.motion', 'Reduce motion'),
      category: msg('sd.category.appearance', 'Appearance'),
      checked: (ctx) => ctx.reducedMotion(),
      run: (ctx) => ctx.toggleReducedMotion(),
    }),
    defineCommand<DemoContext>({
      id: 'sd.pseudo-locale',
      title: msg('sd.pseudo-locale', 'Pseudo-locale'),
      category: msg('sd.category.appearance', 'Appearance'),
      checked: (ctx) => ctx.pseudoLocale(),
      run: (ctx) => ctx.togglePseudoLocale(),
    }),
    defineCommand<DemoContext>({
      id: 'sd.fail',
      title: msg('sd.fail', 'Fail on purpose'),
      category: msg('sd.category.demo', 'Demo'),
      run: () => {
        throw new Error('this command always fails');
      },
    }),
  ],
  keybindings: [defineKeybinding({ command: 'sd.greet', key: 'Mod+Shift+H' }), defineKeybinding({ command: 'sd.compact', key: 'Mod+Alt+K' })],
  menus: [
    defineMenuItem({ menu: 'help', command: 'sd.greet', group: 'demo', order: 100 }),
    defineMenuItem({ menu: 'help', command: 'sd.reset', group: 'demo', order: 200 }),
    defineMenuItem({ menu: 'view', command: 'sd.compact', group: 'notices', order: 100 }),
    defineMenuItem({ menu: 'file', command: 'sd.job.start', group: 'jobs', order: 100 }),
    defineMenuItem({ menu: 'file', command: 'sd.job.quiet', group: 'jobs', order: 200 }),
    // View > Appearance (plan S15): the theme, contrast, motion and the pseudo-locale.
    defineMenuItem({ menu: 'sd.appearance', command: 'sd.theme.system', group: 'theme', order: 100 }),
    defineMenuItem({ menu: 'sd.appearance', command: 'sd.theme.dark', group: 'theme', order: 110 }),
    defineMenuItem({ menu: 'sd.appearance', command: 'sd.theme.light', group: 'theme', order: 120 }),
    defineMenuItem({ menu: 'sd.appearance', command: 'sd.warm-paper', group: 'theme', order: 130 }),
    defineMenuItem({ menu: 'sd.appearance', command: 'sd.contrast', group: 'access', order: 200 }),
    defineMenuItem({ menu: 'sd.appearance', command: 'sd.motion', group: 'access', order: 210 }),
    defineMenuItem({ menu: 'sd.appearance', command: 'sd.pseudo-locale', group: 'language', order: 300 }),
    // In the submenu Help > More demos.
    defineMenuItem({ menu: 'sd.more', command: 'sd.fail', group: 'demo', order: 100 }),
    // The context menu of the greetings count (pd-context-menu menu="sd.greetings"); it may repeat menu-bar commands.
    defineMenuItem({ menu: 'sd.greetings', command: 'sd.greet', group: 'demo', order: 100 }),
    defineMenuItem({ menu: 'sd.greetings', command: 'sd.reset', group: 'demo', order: 200 }),
  ],
  // The pages of the frame (plan S14): each a lazy chunk; Runners brings providers of its own, Lab has a condition.
  pages: [
    definePage({ id: 'sd.home', title: msg('sd.page.home', 'Overview'), icon: 'pd:home', order: 100, load: () => import('./pages/overview-page').then((m) => m.OverviewPage) }),
    definePage<DemoContext>({
      id: 'sd.runners',
      title: msg('sd.page.runners', 'Runners'),
      icon: 'sd:runner',
      order: 150,
      badge: (ctx) => ctx.attention(),
      badgeLabel: (ctx) =>
        ctx.attention() === 1
          ? $localize`:@@sd.runners.offline.one:1 runner offline`
          : $localize`:@@sd.runners.offline.other:${ctx.attention()}:count: runners offline`,
      providers: () => import('./pages/runners.providers').then((m) => m.RUNNERS_PROVIDERS),
      load: () => import('./pages/runners-page').then((m) => m.RunnersPage),
    }),
    definePage({
      id: 'sd.pitwall',
      title: msg('sd.page.pitwall', 'Local AI'),
      icon: 'pd:info',
      order: 180,
      providers: () => import('./pages/pitwall.providers').then((m) => m.PITWALL_PROVIDERS),
      load: () => import('./pages/pitwall-page').then((m) => m.PitwallPage),
    }),
    // Harness pages of the shell's parts: offered in developer mode only (on unless `sd.developer` is "off", as in the
    // reference screenshots, which show a manager app's navigation).
    definePage<DemoContext>({
      id: 'sd.dialogs',
      title: msg('sd.page.dialogs', 'Dialogs'),
      icon: 'pd:window',
      order: 200,
      when: (ctx) => ctx.developer(),
      load: () => import('./pages/dialogs-page').then((m) => m.DialogsPage),
    }),
    definePage<DemoContext>({
      id: 'sd.keys',
      title: msg('sd.page.keys', 'Keyboard'),
      icon: 'pd:keyboard',
      order: 300,
      when: (ctx) => ctx.developer(),
      load: () => import('./pages/keys-page').then((m) => m.KeysPage),
    }),
    definePage<DemoContext>({
      id: 'sd.lab',
      title: msg('sd.page.lab', 'Lab'),
      icon: 'sd:lab',
      order: 500,
      when: (ctx) => ctx.greetings() > 0,
      load: () => import('./pages/lab-page').then((m) => m.LabPage),
    }),
    definePage({ id: 'sd.about', title: msg('sd.page.about', 'About'), icon: 'pd:info', order: 100, section: 'bottom', load: () => import('./pages/about-page').then((m) => m.AboutPage) }),
  ],
  statusItems: [
    defineStatusItem({ id: 'sd.runners', title: msg('sd.status.runners', 'Runners online'), side: 'start', priority: 50, load: () => import('./runners-status').then((m) => m.RunnersStatus) }),
  ],
  settingsPages: DEMO_SETTINGS_PAGES,
  // One dialog of each kind (ADR 0009), plus a modal form and a required choice. Contracts: dialog-contracts.json.
  dialogs: DEMO_DIALOGS,
  healthServices: [
    defineHealthService({ id: 'sd.engine', label: msg('sd.health.engine', 'the demo engine'), icon: 'pd:engine' }),
    defineHealthService({ id: 'sd.index', label: msg('sd.health.index', 'the search index'), icon: 'pd:search' }),
    // The background agent of the desktop demo (`Builder::service`, plan X4): its health entry is kept by paddock-service.
    defineHealthService({ id: 'sd.agent', label: msg('sd.health.agent', 'the demo agent'), icon: 'pd:engine' }),
  ],
  notificationKinds: [defineNotificationKind({ id: 'sd.greeting', title: msg('sd.notification.greeting', 'Greeting'), actions: ['again'] })],
};

/** The demo's submenus (plan S13), registered with `provideSubmenus`. */
export const DEMO_SUBMENUS: readonly SubmenuDef<DemoContext>[] = [
  defineSubmenu<DemoContext>({ id: 'sd.more', menu: 'help', title: msg('sd.menu.more', 'More demos'), group: 'more', order: 900 }),
  defineSubmenu<DemoContext>({ id: 'sd.appearance', menu: 'view', title: msg('sd.menu.appearance', 'Appearance'), group: 'appearance', order: 50 }),
];
