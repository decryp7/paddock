import { defineDialog, defineSettingsPage, msg, type, type ContributionBatch, type DialogDef, type ProvidersLoader, type SettingsPageDef } from '@decryp7/paddock-ui/shell';
import { CHANNEL_EVENTS, type ChannelEventDecl } from './events';
import type { ChannelsCommand } from '../generated/channels-commands';
import { DIALOG_CONTRACTS } from '../generated/dialog-contracts';

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
export const CHANNELS_SECRET_DIALOG_COMMANDS = DIALOG_CONTRACTS[CHANNELS_SECRET_DIALOG_ID].commands satisfies readonly ChannelsCommand[];

export interface ChannelsSettingsPageOptions {
  /** The providers the page boots with: `provideShellChannels` with the app's backend, from a module of its own so they stay lazy. */
  readonly providers: ProvidersLoader;
  /** The page's component: `() => import('@decryp7/paddock-ui/shell/channels/page').then((m) => m.PdChannelsPage)`. */
  readonly load: NonNullable<SettingsPageDef['load']>;
  /**
   * The events this app sends, with a group and a description each. The page then shows grouped checkboxes; names a channel
   * already has that are not listed stay under "Other events". Leave it out to keep the free-text field only.
   */
  readonly events?: readonly ChannelEventDecl[];
}

/** The Notification channels page of the Settings window. */
export function defineChannelsSettingsPage(options: ChannelsSettingsPageOptions): SettingsPageDef {
  const events = options.events ?? [];
  return defineSettingsPage({
    id: CHANNELS_SETTINGS_PAGE,
    group: 'application',
    title: msg('pd.settings.channels', 'Notification channels'),
    order: 85,
    providers: events.length === 0 ? options.providers : async () => [...(await options.providers()), { provide: CHANNEL_EVENTS, useValue: events }],
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
  return defineDialog(DIALOG_CONTRACTS[CHANNELS_SECRET_DIALOG_ID], {
    title: msg('pd.channels.secret.title', 'Channel secret'),
    result: type<boolean>(),
    providers: options.providers,
    load: options.load,
  });
}

/** The id of the remove confirmation (stable: routes, remembered bounds, test ids). */
export const CHANNELS_REMOVE_DIALOG_ID = 'pd.channels.remove';

export interface ChannelsRemoveDialogOptions {
  /** The dialog's component: `() => import('@decryp7/paddock-ui/shell/channels/dialog').then((m) => m.PdChannelRemoveDialog)`. */
  readonly load: DialogDef['load'];
}

/**
 * The remove confirmation (rule 17: a real window): a modal decision about one channel (`inputs: { channelId, name }`), one
 * window per channel. It calls nothing; it resolves `true` when the user chose Remove, `false` or nothing when they kept the channel.
 */
export function defineChannelsRemoveDialog(options: ChannelsRemoveDialogOptions): DialogDef<typeof CHANNELS_REMOVE_DIALOG_ID, boolean, ChannelsCommand> {
  return defineDialog(DIALOG_CONTRACTS[CHANNELS_REMOVE_DIALOG_ID], {
    title: msg('pd.channels.remove.title', 'Remove channel'),
    result: type<boolean>(),
    load: options.load,
  });
}

/** The page and its dialogs as a contribution batch for the app's registry; the app also lists the page among the Settings window's pages. */
export function channelsContributions(page: SettingsPageDef, dialogs: readonly DialogDef[]): ContributionBatch {
  return { settingsPages: [page], dialogs: [...dialogs] };
}
