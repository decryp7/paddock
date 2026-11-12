/**
 * `@decryp7/paddock-ui/shell/contributions`: the contribution registry on its own (ADR 0012 §2): `define*`, `provideContributions`,
 * the registry, the typed context `ShellContext<T>` and the command service. It imports no Material and not the `@decryp7/paddock-ui`
 * root, so an app that only declares contributions does not pull the frame, the theme or Material into its first paint.
 * `@decryp7/paddock-ui/shell` re-exports all of it.
 */
export { msg, isMessage, localizedText, CONTRIBUTION_ID, type Message } from './src/message';
export { type ShellTypes, type AppCommandName, type IpcCommandName, type TypeToken, type, type DialogOutcome, DISMISSED, answered, answerOf } from './src/types';
export { KEY_MODIFIERS, parseKey, normalizeKey, pressedKeys, toAccelerator, type KeyModifier, type ParsedKey } from './src/keys';
export { loadPart, type ComponentLoader, type ProvidersLoader, type LazyPart, type LoadedPart } from './src/lazy';
export {
  defineCommand,
  defineKeybinding,
  defineMenuItem,
  definePage,
  defineStatusItem,
  defineSettingsPage,
  defineDialog,
  defineHealthService,
  defineNotificationKind,
  SHELL_MENUS,
  DIALOG_KINDS,
  KIND_MODALITIES,
  DIALOG_MIN,
  VIEWER_MIN,
  KIND_MIN_SIZE,
  type Live,
  type CommandDef,
  type KeybindingDef,
  type MenuItemDef,
  type ShellMenu,
  type PageDef,
  type StatusItemDef,
  type SettingsGroup,
  type SettingsPageDef,
  type DialogKind,
  type DialogModality,
  type DialogContractData,
  type DialogUi,
  type DialogDef,
  type HealthServiceDef,
  type NotificationKindDef,
} from './src/define';
export {
  defineContributionKind,
  SHELL_KINDS,
  COMMANDS,
  KEYBINDINGS,
  MENUS,
  PAGES,
  STATUS_ITEMS,
  SETTINGS_PAGES,
  DIALOGS,
  HEALTH_SERVICES,
  NOTIFICATION_KINDS,
  type ContributionKind,
  type ContributionLookup,
} from './src/kinds';
export {
  provideContributions,
  provideContributionKinds,
  contributionProviders,
  type ContributionBatch,
} from './src/provide';
export { SHELL_CONTRIBUTIONS, SHELL_CONTRIBUTION_KINDS, type ContributionEntry } from './src/tokens';
export { ContributionRegistry, safeIdentity } from './src/registry';
export {
  provideShellContext,
  describeError,
  ShellContextHost,
  SHELL_PORTS,
  SHELL_IDLE,
  SHELL_COMMAND_RUNNER,
  APP_QUIT,
  DEFAULT_APP_QUIT,
  type ShellContext,
  type ShellCore,
  type ShellPorts,
  type ShellNotice,
  type AppContextFactory,
  type ContextPrefetch,
} from './src/context';
export { CommandService, evaluate, provideShellCore } from './src/commands';
