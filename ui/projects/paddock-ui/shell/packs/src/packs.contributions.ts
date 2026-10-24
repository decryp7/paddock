import { defineSettingsPage, msg, type ContributionBatch, type ProvidersLoader, type SettingsPageDef } from '@decryp7/paddock-ui/shell';
import type { PacksCommand } from '../generated/packs-commands';

/** The id of the Knowledge packs settings page. */
export const PACKS_SETTINGS_PAGE = 'pd.packs';

/**
 * What the Settings window may call for this page, on top of the settings commands: the Settings dialog's contract lists
 * these too (Rust's guard allows exactly the contract; write them out in the dialog, the contract check reads literals).
 */
export const PACKS_SETTINGS_COMMANDS = ['packs_status', 'packs_reload'] as const satisfies readonly PacksCommand[];

export interface PacksSettingsPageOptions {
  /** The providers the page boots with: the app's `provideShellPacks` with its backend, from a module of its own so they stay lazy. */
  readonly providers: ProvidersLoader;
  /** The page's component: `() => import('@decryp7/paddock-ui/shell/packs/page').then((m) => m.PdPacksPage)`. */
  readonly load: NonNullable<SettingsPageDef['load']>;
}

/** The Knowledge packs page of the Settings window: the packs in use, what was refused and why, and Reload. */
export function definePacksSettingsPage(options: PacksSettingsPageOptions): SettingsPageDef {
  return defineSettingsPage({
    id: PACKS_SETTINGS_PAGE,
    group: 'advanced',
    title: msg('pd.settings.packs', 'Knowledge packs'),
    order: 60,
    providers: options.providers,
    load: options.load,
  });
}

/** The page as a contribution batch for the app's registry; the app also lists it among the Settings window's pages. */
export function packsContributions(page: SettingsPageDef): ContributionBatch {
  return { settingsPages: [page] };
}
