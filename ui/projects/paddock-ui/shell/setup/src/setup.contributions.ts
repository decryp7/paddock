import { defineCommand, defineDialog, defineMenuItem, defineSettingsPage, msg, type ContributionBatch, type DialogDef, type ProvidersLoader, type SettingsPageDef } from '@decryp7/paddock-ui/shell';
import { SETUP_PAGE_ID } from '../generated/setup';
import type { SetupCommand } from '../generated/setup-commands';
import { DIALOG_CONTRACTS } from '../generated/dialog-contracts';

// The set-up part's contributions: static data and loaders only (lazy by construction, plan §9). The checklist's window and
// the Settings page, and the providers they need, are chunks of their own, loaded when they open.

/** The id of the Set-up window (a dialog of the app's registry, so its contract is there). */
export const SETUP_DIALOG_ID = 'pd.setup';

/** The id of the Set-up settings page. */
export const SETUP_SETTINGS_PAGE = SETUP_PAGE_ID;

/** What the checklist may call: the Set-up window's contract and, in the Settings dialog's, the Set-up page's. */
export const SETUP_DIALOG_COMMANDS = DIALOG_CONTRACTS[SETUP_DIALOG_ID].commands satisfies readonly SetupCommand[];

export interface SetupWindowOptions {
  /**
   * The providers the Set-up window and the Settings page boot with: the app's `provideShellSetup` with its backend
   * (`TauriSetupBackend` over its IPC), from a module of its own so they stay lazy.
   */
  readonly providers: ProvidersLoader;
  /**
   * The Settings page's component, from the registry (the entry point cannot import the page's: it is a chunk of its own):
   * `() => import('@decryp7/paddock-ui/shell/setup/page').then((m) => m.PdSetupPage)`.
   */
  readonly loadPage: NonNullable<SettingsPageDef['load']>;
  /**
   * The window's component: `() => import('@decryp7/paddock-ui/shell/setup/page').then((m) => m.PdSetupDialog)`.
   */
  readonly loadDialog: NonNullable<DialogDef['load']>;
}

/**
 * The Set-up window (ADR 0009 kind `tool`: modeless, so the user can answer an OS prompt or open Pitwall beside it; it
 * remembers its size) and the Set-up page of the Settings window: the same checklist, the way back to it later.
 */
export function defineSetup(options: SetupWindowOptions): { readonly dialog: DialogDef; readonly page: SettingsPageDef } {
  return {
    dialog: defineDialog(DIALOG_CONTRACTS[SETUP_DIALOG_ID], {
      title: msg('pd.setup.title', 'Set-up'),
      providers: options.providers,
      load: options.loadDialog,
    }),
    page: defineSettingsPage({
      id: SETUP_SETTINGS_PAGE,
      group: 'application',
      title: msg('pd.settings.setup', 'Set-up'),
      order: 90,
      providers: options.providers,
      load: options.loadPage,
    }),
  };
}

/** The Help > Set-up… command, which opens the window. The app lists the dialog and the page (from `defineSetup`) itself. */
export const SETUP_CONTRIBUTIONS: ContributionBatch = {
  commands: [
    defineCommand({
      id: 'pd.setup.open',
      title: msg('pd.setup.open', 'Set-up…'),
      category: msg('pd.category.help', 'Help'),
      run: (ctx) => void ctx.openDialog(SETUP_DIALOG_ID),
    }),
  ],
  menus: [defineMenuItem({ menu: 'help', command: 'pd.setup.open', group: 'about', order: 300 })],
};
