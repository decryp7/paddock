/**
 * Public API for Debrief UI plugins.
 *
 * A plugin is a set of contributions in the shell's registry (`SHELL_CONTRIBUTIONS`, workbench.md §4.3 M1), registered
 * in `app.config.ts` with one call:
 *
 * ```ts
 * provideDebriefPlugin({ views, commands, inspectorSections, fieldFormatters })
 * ```
 *
 * - `DebriefView`            dockable panel, loaded lazily, offered while `when(summary)` holds
 * - `DebriefCommand`         palette / menu action with optional key binding; receives a `CommandContext`
 *                            (`ShellContext<DebriefContext>`); held by the registry as the shell's `CommandDef`
 * - `DebriefInspectorSection` extra block in the event inspector (component with a `record` input)
 * - `DebriefFieldFormatter`  display formatting for fields in the grid and inspector
 * - `DebriefActivityItem`    button in the left activity bar: shows a view, runs a command or opens a fly-out menu
 * - `DebriefEvents`          read-only streams of workspace / query / selection / profile changes
 *
 * Plugins never touch the stores: they observe through `DebriefEvents` and act through `CommandContext`.
 */
export { ActivityItems, viewCategoryMenu } from './activity-items';
export { CommandRegistry, COMMAND_CONTEXT, provideDebriefShellCore } from './command-registry';
export { combineContext, debriefCommandContext, JUMP_TIMEOUT_MS, MAIN_WORKSPACE, WorkspaceCommandContext } from './command-context';
export { DebriefEvents, StoreDebriefEvents } from './events';
export { EVENT_ID_FORMATTER, FieldFormatters, findFormatter } from './field-formatters';
export { fuzzyFilter, fuzzyScore } from './fuzzy';
export { InspectorSections } from './inspector-sections';
export { formatKeybinding, matchesKeybinding } from './keybinding';
export { provideDebriefPlugin, UI_PLUGIN_API_VERSION, type DebriefPlugin } from './provide';
export type { ViewLayout } from '../layout-plan/layout-plan';
export {
  contributeFrom,
  debriefContributionProviders,
  provideDebriefContributions,
  provideDebriefKinds,
  DEBRIEF_KIND,
  DEBRIEF_KINDS,
  type DebriefCommandDef,
  type DebriefContributions,
} from './contributions';
export type { DebriefContext, EventId, TimeDisplay, TimeRange, WorkspaceHandle } from './context';
export {
  flattenContributions,
  type CommandContext,
  type DebriefActivityItem,
  type DebriefActivityMenuItem,
  type DebriefActivityTarget,
  type DebriefCommand,
  type DebriefFieldFormatter,
  type DebriefInspectorSection,
  type DebriefView,
  type ViewCategory,
  type ViewPosition,
} from './tokens';
export { isViewAvailable, ViewRegistry } from './view-registry';
