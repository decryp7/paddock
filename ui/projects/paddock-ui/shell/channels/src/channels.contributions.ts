import { defineDialog, defineSettingsPage, msg, type, type ContributionBatch, type DialogDef, type ProvidersLoader, type SettingsPageDef } from '@decryp7/paddock-ui/shell';
import type { ChannelsCommand } from '../generated/channels-commands';

// The channels part's contributions: static data and loaders only (lazy by construction, plan §9). The page's component, the
// secret dialog's component and the providers each need are chunks of their own, loaded when their window opens.

/** The id of the Notification channels settings page. */
export const CHANNELS_SETTINGS_PAGE = 'pd.channels';

/** The id of the secret dialog (stable: routes, remembered bounds, test ids). */
export const CHANNELS_SECRET_DIALOG_ID = 'pd.channels.secret';

/**
 * What the Settings window may call for this page, on top of the settings commands: the Settings dialog's contract lists
 * these too (write them out in the dialog, the contract check reads literals). `channels_set_secret` is not among them: the
 * secret goes from the secret dialog's window only.
 */
export const CHANNELS_SETTINGS_COMMANDS = ['channels_get', 'channels_save', 'channels_clear_secret', 'channels_test'] as const satisfies readonly ChannelsCommand[];

/** What the secret dialog's window may call (its contract; Rust's guard allows exactly this). */
export const CHANNELS_SECRET_DIALOG_COMMANDS = ['channels_set_secret'] as const satisfies readonly ChannelsCommand[];

export interface ChannelsSettingsPageOptions {
  /** The providers the page boots with: `provideShellChannels` with the app's backend, from a module of its own so they stay lazy. */
  readonly providers: ProvidersLoader;
  /** The page's component: `() => import('@decryp7/paddock-ui/shell/channels/page').then((m) => m.PdChannelsPage)`. */
  readonly load: NonNullable<SettingsPageDef['load']>;
}

/** The Notification channels page of the Settings window. */
export function defineChannelsSettingsPage(options: ChannelsSettingsPageOptions): SettingsPageDef {
  return defineSettingsPage({
    id: CHANNELS_SETTINGS_PAGE,
    group: 'application',
    title: msg('pd.settings.channels', 'Notification channels'),
    order: 85,
    providers: options.providers,
    load: options.load,
  });
}

export interface ChannelsSecretDialogOptions {
  /** The providers the dialog's window boots with: `provideChannelsBackend` with the app's backend. */
  readonly providers: ProvidersLoader;
  /** The dialog's component: `() => import('@decryp7/paddock-ui/shell/channels/dialog').then((m) => m.PdChannelSecretDialog)`. */
  readonly load: DialogDef['load'];
}

/**
 * The secret dialog (a modal form): it takes the secret of one channel (`inputs: { channelId, label, kind }`) and sends it
 * to the engine; it resolves `true` when the engine stored it, nothing when dismissed. Opened with
 * `DialogService.open(CHANNELS_SECRET_DIALOG_ID, { inputs })` from the page.
 */
export function defineChannelsSecretDialog(options: ChannelsSecretDialogOptions): DialogDef<typeof CHANNELS_SECRET_DIALOG_ID, boolean, ChannelsCommand> {
  return defineDialog({
    id: CHANNELS_SECRET_DIALOG_ID,
    kind: 'form',
    modality: 'modal',
    requiredChoice: false,
    title: msg('pd.channels.secret.title', 'Channel secret'),
    width: 440,
    result: type<boolean>(),
    commands: CHANNELS_SECRET_DIALOG_COMMANDS,
    providers: options.providers,
    load: options.load,
  });
}

/** The page as a contribution batch for the app's registry; the app also lists it among the Settings window's pages, and the dialog among its dialogs. */
export function channelsContributions(page: SettingsPageDef, dialog: DialogDef): ContributionBatch {
  return { settingsPages: [page], dialogs: [dialog] };
}
