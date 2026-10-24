/**
 * `@decryp7/paddock-ui/shell`: the shell core. Contributions (`define*`, `provideContributions`), the registry, the
 * typed context `ShellContext<T>`, the command service and the manifest Rust checks (ADR 0012 §2; plan S2, §9).
 * The frame parts and widgets arrive with plan S14.
 */
export { msg, isMessage, CONTRIBUTION_ID, type Message } from './src/message';
export { type ShellTypes, type IpcCommandName, type TypeToken, type } from './src/types';
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
  type DialogSpec,
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
  ShellContextHost,
  SHELL_PORTS,
  SHELL_IDLE,
  SHELL_COMMAND_RUNNER,
  type ShellContext,
  type ShellCore,
  type ShellPorts,
  type ShellNotice,
  type AppContextFactory,
  type ContextPrefetch,
} from './src/context';
export { CommandService, evaluate, provideShellCore } from './src/commands';
export {
  buildManifest,
  serializeManifest,
  parseManifest,
  manifestDrift,
  MANIFEST_VERSION,
  MANIFEST_FILE,
  type PaddockManifest,
  type ManifestCommand,
  type ManifestKeybinding,
  type ManifestMenuItem,
  type ManifestDialog,
  type ManifestNotificationKind,
  type ManifestParse,
} from './src/manifest';
