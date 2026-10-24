import { defineDialog, defineSettingsPage, msg, type, type ContributionBatch, type DialogDef, type ProvidersLoader, type SettingsPageDef } from '@decryp7/paddock-ui/shell';
import type { StorageCommand } from '../generated/storage-commands';

// The storage part's contributions: static data and loaders only (lazy by construction, plan §9). The page's component, the
// confirmation dialog's component and the providers are chunks of their own, loaded when their window opens.

/** The id of the Storage settings page. */
export const STORAGE_SETTINGS_PAGE = 'pd.storage';

/** The id of the clear confirmation (stable: routes, remembered bounds, test ids). */
export const STORAGE_CONFIRM_DIALOG_ID = 'pd.storage.confirm';

/**
 * What the Settings window may call for this page, on top of the settings commands: the Settings dialog's contract lists
 * these too (write them out in the dialog, the contract check reads literals).
 */
export const STORAGE_SETTINGS_COMMANDS = ['storage_last', 'storage_scan', 'storage_clear', 'storage_cancel', 'storage_reveal'] as const satisfies readonly StorageCommand[];

export interface StorageSettingsPageOptions {
  /** The providers the page boots with: `provideShellStorage` with the app's backend, from a module of its own so they stay lazy. */
  readonly providers: ProvidersLoader;
  /** The page's component: `() => import('@decryp7/paddock-ui/shell/storage/page').then((m) => m.PdStoragePage)`. */
  readonly load: NonNullable<SettingsPageDef['load']>;
}

/** The Storage page of the Settings window: what the app keeps, per folder, and Clear with a confirmation. */
export function defineStorageSettingsPage(options: StorageSettingsPageOptions): SettingsPageDef {
  return defineSettingsPage({
    id: STORAGE_SETTINGS_PAGE,
    group: 'application',
    title: msg('pd.settings.storage', 'Storage'),
    order: 90,
    providers: options.providers,
    load: options.load,
  });
}

export interface StorageConfirmDialogOptions {
  /** The dialog's component: `() => import('@decryp7/paddock-ui/shell/storage/dialog').then((m) => m.PdStorageConfirmDialog)`. */
  readonly load: DialogDef['load'];
}

/**
 * The clear confirmation (rule 17: a real window): a modal decision about what is about to be removed (`inputs: { what, bytes,
 * data }`), one window per question. It calls nothing; it resolves `true` when the user chose Clear, `false` or nothing when they
 * kept everything. Keep is the default; Clear is the danger action.
 */
export function defineStorageConfirmDialog(options: StorageConfirmDialogOptions): DialogDef<typeof STORAGE_CONFIRM_DIALOG_ID, boolean, StorageCommand> {
  return defineDialog({
    id: STORAGE_CONFIRM_DIALOG_ID,
    kind: 'decision',
    title: msg('pd.storage.confirm.title', 'Clear storage'),
    width: 440,
    instanced: true,
    result: type<boolean>(),
    commands: [],
    load: options.load,
  });
}

/** The page and its dialog as a contribution batch for the app's registry; the app also lists the page among the Settings window's pages. */
export function storageContributions(page: SettingsPageDef, dialogs: readonly DialogDef[]): ContributionBatch {
  return { settingsPages: [page], dialogs: [...dialogs] };
}
