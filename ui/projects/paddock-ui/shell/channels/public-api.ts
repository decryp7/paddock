/**
 * `@decryp7/paddock-ui/shell/channels`: notification channels (plan X17) over `paddock-channels`' Tauri adapter
 * (`channels_get`, `channels_save`, `channels_set_secret`, `channels_clear_secret`, `channels_test`, and the outcome of a test
 * on `paddock://channels`): the `ChannelsBackend` port (Tauri, and in-memory), the per-window store, the draft helpers and the
 * Settings > Notification channels page and secret dialog definitions.
 *
 * The page never depends on anything but the engine (rule 19). A secret (token or password) is typed in the secret dialog's
 * window and goes straight to the engine; it is in no store, event, log or URL. A test returns at once; its outcome (it can
 * take a minute) arrives as an event and is never timed out (rules 10, 12).
 *
 * The page (`PdChannelsPage`, `@decryp7/paddock-ui/shell/channels/page`) and the dialog (`PdChannelSecretDialog`,
 * `@decryp7/paddock-ui/shell/channels/dialog`, with the remove confirmation `PdChannelRemoveDialog`) are chunks of their own that only their windows load.
 *
 * ```ts
 * export const CHANNELS_PAGE = defineChannelsSettingsPage({
 *   providers: () => import('./channels.providers').then((m) => m.CHANNELS_PROVIDERS),
 *   load: () => import('@decryp7/paddock-ui/shell/channels/page').then((m) => m.PdChannelsPage),
 * });
 * export const CHANNELS_SECRET_DIALOG = defineChannelsSecretDialog({
 *   providers: () => import('./channels.providers').then((m) => m.CHANNELS_SECRET_PROVIDERS),
 *   load: () => import('@decryp7/paddock-ui/shell/channels/dialog').then((m) => m.PdChannelSecretDialog),
 * });
 * // the registry: settingsPages: [..., CHANNELS_PAGE], dialogs: [..., CHANNELS_SECRET_DIALOG];
 * // the Settings dialog: commands [...SETTINGS_DIALOG_COMMANDS, ...CHANNELS_SETTINGS_COMMANDS]
 * ```
 */
export { ChannelsBackend, TauriChannelsBackend, type ChannelsIpc, type Unlisten } from './src/backend';
export { MemoryChannelsBackend, settingsProblem, type MemoryChannelsOptions, type TestOutcome } from './src/memory';
export { parseChannel, parseChannelsView, parseChannelsEvent, parseTestStarted, asChannelsError, channelsMessage } from './src/wire';
export { ChannelsStore, type ChannelsStoreType, type TestState, type TestProgress, type SaveStatus } from './src/channels.store';
export { CHANNEL_EVENTS, groupEvents, unknownEvents, toggleEvent, type ChannelEventDecl, type ChannelEventGroup } from './src/events';
export {
  EMPTY_DRAFT,
  channelNeedsSecret,
  secretKind,
  tokens,
  draftFromChannel,
  draftToChannel,
  draftHints,
  newChannelId,
  type ChannelDraft,
  type DraftField,
} from './src/draft';
export {
  CHANNELS_SETTINGS_PAGE,
  CHANNELS_SECRET_DIALOG_ID,
  CHANNELS_REMOVE_DIALOG_ID,
  CHANNELS_SETTINGS_COMMANDS,
  CHANNELS_SECRET_DIALOG_COMMANDS,
  defineChannelsSettingsPage,
  defineChannelsSecretDialog,
  defineChannelsRemoveDialog,
  channelsContributions,
  type ChannelsSettingsPageOptions,
  type ChannelsSecretDialogOptions,
  type ChannelsRemoveDialogOptions,
} from './src/channels.contributions';
export { provideShellChannels, provideChannelsBackend, type ShellChannelsOptions } from './src/provide-channels';
// The wire types, the event and the command names (a mirror of paddock-channels' adapter until it generates them).
export * from './generated/channels';
export { CHANNELS_COMMANDS, type ChannelsCommand } from './generated/channels-commands';
