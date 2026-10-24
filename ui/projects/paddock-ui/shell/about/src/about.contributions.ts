import {
  defineCommand,
  defineDialog,
  defineMenuItem,
  defineNotificationKind,
  msg,
  type ContributionBatch,
  type DialogDef,
} from '@decryp7/paddock-ui/shell';
import type { ProvidersLoader } from '@decryp7/paddock-ui/shell';
import type { AboutCommand } from '../generated/about-commands';

// The About part's contributions: static data and loaders only (lazy by construction, plan §9). Each dialog's component
// and the providers its window needs are chunks of their own.

/** The dialog ids (stable: routes, remembered bounds, test ids). */
export const ABOUT_DIALOG_IDS = { about: 'pd.about', updates: 'pd.updates', whatsNew: 'pd.whats-new' } as const;

/** What each dialog's window may call (its contract; Rust's guard allows exactly these). */
export const ABOUT_DIALOG_COMMANDS = {
  about: ['about_info', 'about_legal'],
  updates: [
    'updates_status',
    'updates_check',
    'updates_download',
    'updates_cancel',
    'updates_install_on_quit',
    'updates_restart',
    'updates_set_preferences',
  ],
  whatsNew: ['about_whats_new'],
} as const satisfies Record<keyof typeof ABOUT_DIALOG_IDS, readonly AboutCommand[]>;

export interface AboutDialogsOptions {
  /**
   * The providers a dialog window of About, Updates or What's New boots with: the app's `provideShellAbout` with its
   * backends (`TauriAboutBackend`/`TauriUpdatesBackend` over its IPC), from a module of its own so they stay lazy.
   */
  readonly providers: ProvidersLoader;
}

/** The three dialogs (ADR 0009 kinds): About and What's New are viewers, Updates is a tool kept open beside the work. */
export function defineAboutDialogs(options: AboutDialogsOptions): readonly [DialogDef, DialogDef, DialogDef] {
  return [
    defineDialog({
      id: ABOUT_DIALOG_IDS.about,
      kind: 'viewer',
      title: msg('pd.about.title', 'About'),
      commands: ABOUT_DIALOG_COMMANDS.about,
      providers: options.providers,
      load: () => import('./about-dialog').then((m) => m.PdAboutDialog),
    }),
    defineDialog({
      id: ABOUT_DIALOG_IDS.updates,
      kind: 'tool',
      title: msg('pd.updates.title', 'Updates'),
      width: 560,
      height: 480,
      commands: ABOUT_DIALOG_COMMANDS.updates,
      providers: options.providers,
      load: () => import('./updates-dialog').then((m) => m.PdUpdatesDialog),
    }),
    defineDialog({
      id: ABOUT_DIALOG_IDS.whatsNew,
      kind: 'viewer',
      title: msg('pd.whats-new.title', 'What’s new'),
      commands: ABOUT_DIALOG_COMMANDS.whatsNew,
      providers: options.providers,
      load: () => import('./whats-new-dialog').then((m) => m.PdWhatsNewDialog),
    }),
  ];
}

/** The commands, Help menu items and notification kind of About, Updates and What's New, with the app's dialogs. */
export function aboutContributions(dialogs: readonly DialogDef[]): ContributionBatch {
  const category = msg('pd.category.help', 'Help');
  return {
    commands: [
      defineCommand({
        id: 'pd.about.open',
        title: msg('pd.about.open', 'About'),
        category,
        run: (ctx) => void ctx.openDialog(ABOUT_DIALOG_IDS.about),
      }),
      defineCommand({
        id: 'pd.updates.open',
        title: msg('pd.updates.open', 'Check for updates…'),
        category,
        run: (ctx) => void ctx.openDialog(ABOUT_DIALOG_IDS.updates),
      }),
      defineCommand({
        id: 'pd.whats-new.open',
        title: msg('pd.whats-new.open', 'What’s new'),
        category,
        run: (ctx) => void ctx.openDialog(ABOUT_DIALOG_IDS.whatsNew),
      }),
    ],
    menus: [
      defineMenuItem({ menu: 'help', command: 'pd.whats-new.open', group: 'about', order: 100 }),
      defineMenuItem({ menu: 'help', command: 'pd.updates.open', group: 'about', order: 200 }),
      defineMenuItem({ menu: 'help', command: 'pd.about.open', group: 'about', order: 300 }),
    ],
    dialogs: dialogs,
    notificationKinds: [
      defineNotificationKind({ id: 'pd.updates.available', title: msg('pd.updates.notification.available', 'Update available'), actions: [] }),
    ],
  };
}
