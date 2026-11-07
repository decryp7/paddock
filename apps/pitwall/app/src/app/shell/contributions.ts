import {
  defineCommand,
  defineDialog,
  defineMenuItem,
  definePage,
  defineSettingsPage,
  defineStatusItem,
  msg,
  type,
  type ContributionBatch,
} from '@decryp7/paddock-ui/shell';
import { aboutContributions, defineAboutDialogs } from '@decryp7/paddock-ui/shell/about';
import { defineConfirmDialog } from '@decryp7/paddock-ui/shell/confirm';
import { defineDiagnosticsDialog, diagnosticsContributions } from '@decryp7/paddock-ui/shell/diagnostics';
import { DIALOG_CONTRACTS } from '../generated/dialog-contracts';
import type { ConsentAnswer } from '../features/service/service-consent';
import type { QuitAnswer } from '../features/quit/quit-dialog';
import type { PitwallContext } from './context';
import { NAV } from './nav';

// The registration file of Pitwall: static data and loaders only (lazy by construction). Its lints run in
// contributions.spec.ts (`assertContributionsValid`, the manifest and the dialog contracts).

/**
 * The Settings window: the shell's window as Pitwall's own dialog, since its pages are the app's choice (the contract is in
 * `src-tauri/src/dialogs.rs`). A tool: modeless, large, remembers its size. Pitwall's page is the one below; Appearance and
 * Updates are the shell's.
 */
export const SETTINGS_DIALOG = defineDialog(DIALOG_CONTRACTS['pd.settings'], {
  title: msg('pd.settings.title', 'Settings'),
  providers: () => import('./settings-window').then((m) => m.SETTINGS_WINDOW),
  load: () => import('@decryp7/paddock-ui/shell/settings/window').then((m) => m.PdSettingsWindow),
});

/** Pitwall's page of the Settings window: first, before the shell's Appearance (50), since it is what Settings is mostly for. */
export const SETTINGS_PAGES = [
  defineSettingsPage({
    id: 'pw.general',
    group: 'application',
    title: msg('pw.settings.general', 'Pitwall'),
    order: 10,
    load: () => import('../features/settings/settings-page').then((m) => m.SettingsPage),
  }),
  // Which widgets the tray popup shows (family ADR 0035); "Customize widgets…" in the popup opens Settings here.
  // The apps whose tools the Playground chat may use (ADR 0023 Decision 7). In a build without the chat it says so.
  defineSettingsPage({
    id: 'pw.connected-apps',
    group: 'application',
    title: msg('pw.settings.connected-apps', 'Connected apps'),
    order: 15,
    load: () => import('../features/connected-apps/connected-apps-page').then((m) => m.ConnectedAppsPage),
  }),
  defineSettingsPage({
    id: 'pw.tray',
    group: 'application',
    title: msg('pw.settings.tray', 'Tray popup'),
    order: 20,
    load: () => import('../features/settings/tray-widgets-page').then((m) => m.TrayWidgetsPage),
  }),
] as const;

/** "New app" (a modal form). Answers the app's name. */
export const CREATE_APP_DIALOG = defineDialog(DIALOG_CONTRACTS['pw.create-app'], {
  title: msg('pw.create-app.title', 'New app'),
  result: type<string>(),
  load: () => import('../features/apps/create-app-dialog').then((m) => m.CreateAppDialog),
});

/** "Connect <app>" (a modal form): the AI access key the user pastes. Answers the key, trimmed. */
export const MCP_KEY_DIALOG = defineDialog(DIALOG_CONTRACTS['pw.mcp-key'], {
  title: msg('pw.mcp-key.title', 'Connect app'),
  result: type<string>(),
  load: () => import('../features/connected-apps/mcp-key-dialog').then((m) => m.McpKeyDialog),
});

/** A new key, shown once (a required choice). Answers true once the user saved it. */
export const KEY_SECRET_DIALOG = defineDialog(DIALOG_CONTRACTS['pw.key-secret'], {
  title: msg('pw.key-secret.title', 'New key'),
  result: type<true>(),
  load: () => import('../features/apps/key-secret-dialog').then((m) => m.KeySecretDialog),
});

/** The quit question, "Keep serving after you quit?" in a window titled "Quit Pitwall" (a decision). Answers whether to stop serving and whether to keep the choice. */
export const QUIT_DIALOG = defineDialog(DIALOG_CONTRACTS['pw.quit'], {
  title: msg('pw.quit.title', 'Quit Pitwall'),
  result: type<QuitAnswer>(),
  load: () => import('../features/quit/quit-dialog').then((m) => m.QuitDialog),
});

/** "Run Pitwall's server in the background?" (a decision). Answers install, or not now and maybe never again. */
export const CONSENT_DIALOG = defineDialog(DIALOG_CONTRACTS['pw.service-consent'], {
  title: msg('pw.service-consent.title', 'Run in the background'),
  result: type<ConsentAnswer>(),
  load: () => import('../features/service/service-consent').then((m) => m.ServiceConsentDialog),
});

/** The shell's About, Updates and What's New; their windows boot Pitwall's About providers. */
export const ABOUT_DIALOGS = defineAboutDialogs({ providers: () => import('./about.providers').then((m) => m.ABOUT_PROVIDERS) });

/** The shared confirmation (ADR 0026 §5): every delete, revoke, clear, roll back and download asks with it. */
export const CONFIRM_DIALOG = defineConfirmDialog({
  load: () => import('@decryp7/paddock-ui/shell/confirm/dialog').then((m) => m.PdConfirmDialog),
});

/** Report a problem (Help menu, command palette): shows every file with its size before saving; its window boots the diagnostics providers. */
export const REPORT_PROBLEM_DIALOG = defineDiagnosticsDialog({ providers: () => import('./diagnostics.providers').then((m) => m.DIAGNOSTICS_PROVIDERS) });

export const DIALOGS = [CONFIRM_DIALOG, SETTINGS_DIALOG, CREATE_APP_DIALOG, MCP_KEY_DIALOG, KEY_SECRET_DIALOG, QUIT_DIALOG, CONSENT_DIALOG, ...ABOUT_DIALOGS, REPORT_PROBLEM_DIALOG] as const;

/** Report a problem's command, Help menu item and the Logging settings page. */
export const DIAGNOSTICS_CONTRIBUTIONS = diagnosticsContributions([]);

/** About's commands, Help menu items and notice kind. */
export const ABOUT_CONTRIBUTIONS = aboutContributions([]);

/**
 * Every screen of the sidebar, in order. Each page is `pw-page` (the service's notices and the running jobs above the
 * screen) around the screen's own component, whose chunk is loaded first.
 */
const PAGES = NAV.map((item, index) =>
  definePage<PitwallContext>({
    id: `pw.${item.id}`,
    title: msg(`pw.page.${item.id}`, item.label),
    icon: `pw:${item.icon}`,
    order: (index + 1) * 100,
    load: () => item.load().then(() => import('./page').then((m) => m.PitwallPage)),
    ...(item.id === 'apps'
      ? { badge: (ctx: PitwallContext) => ctx.pairings() || null, badgeLabel: (ctx: PitwallContext) => (ctx.pairings() === 1 ? '1 pairing request' : `${ctx.pairings()} pairing requests`) }
      : item.id === 'models'
        ? { badge: (ctx: PitwallContext) => ctx.downloading() || null, badgeLabel: (ctx: PitwallContext) => (ctx.downloading() === 1 ? '1 download running' : `${ctx.downloading()} downloads running`) }
        : {}),
  }),
);

const SERVER = msg('pw.category.server', 'Server');
const APP = msg('pd.category.app', 'Application');

export const CONTRIBUTIONS: ContributionBatch<PitwallContext> = {
  commands: [
    defineCommand<PitwallContext>({
      id: 'pw.pause-serving',
      title: msg('pw.pause-serving', 'Pause serving'),
      category: SERVER,
      when: (ctx) => ctx.serving(),
      enabled: (ctx) => !ctx.busy('serving'),
      run: (ctx) => ctx.setServing(true),
    }),
    defineCommand<PitwallContext>({
      id: 'pw.resume-serving',
      title: msg('pw.resume-serving', 'Resume serving'),
      category: SERVER,
      when: (ctx) => ctx.paused(),
      enabled: (ctx) => !ctx.busy('serving'),
      run: (ctx) => ctx.setServing(false),
    }),
    defineCommand<PitwallContext>({
      id: 'pw.release-memory',
      title: msg('pw.release-memory', 'Release memory'),
      category: SERVER,
      enabled: (ctx) => !ctx.busy('release'),
      run: (ctx) => ctx.releaseMemory(),
    }),
    defineCommand<PitwallContext>({
      id: 'pw.quit-stop',
      title: msg('pw.quit-stop', 'Quit Pitwall and stop serving'),
      category: APP,
      run: (ctx) => ctx.quit('stopServing'),
    }),
  ],
  menus: [
    defineMenuItem<PitwallContext>({ menu: 'app', command: 'pw.quit-stop', group: 'quit', order: 900 }),
  ],
  pages: PAGES,
  statusItems: [
    defineStatusItem<PitwallContext>({
      id: 'pw.serving',
      title: msg('pw.status.serving', 'The Pitwall server'),
      side: 'start',
      priority: 800,
      pinned: true,
      load: () => import('./status-items').then((m) => m.ServingStatusItem),
    }),
    defineStatusItem<PitwallContext>({
      id: 'pw.memory',
      title: msg('pw.status.memory', 'Memory in use'),
      side: 'end',
      priority: 600,
      load: () => import('./status-items').then((m) => m.MemoryStatusItem),
    }),
  ],
  settingsPages: SETTINGS_PAGES,
  dialogs: DIALOGS,
};
