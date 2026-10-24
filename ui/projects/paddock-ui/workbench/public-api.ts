/**
 * `@decryp7/paddock-ui/workbench`: the workbench core (ADR 0013; plan W1). The contribution kinds an IDE-style app adds to
 * the shell's one registry (`defineView`, `defineActivityItem`, `defineExplorerProvider`, `definePaletteProvider`,
 * `definePerspective`, `defineSharedState`, `defineAssistantContext`), `provideWorkbenchContributions`, the typed
 * `WorkbenchRegistry`, panel ids for view instances, and the `WORKBENCH_PORTS` the frame provides so the parts never
 * import the dock. The lint is the shell's `assertContributionsValid`, which checks these kinds too.
 */
export {
  defineView,
  defineActivityItem,
  defineExplorerProvider,
  definePaletteProvider,
  definePerspective,
  defineSharedState,
  defineAssistantContext,
  VIEW_AREAS,
  type ViewDef,
  type ActivityItemDef,
  type ActivityMenuEntry,
  type ActivityTarget,
  type ExplorerNode,
  type ExplorerRequest,
  type ExplorerPage,
  type ExplorerAction,
  type ExplorerDragData,
  type ExplorerProviderDef,
  type PaletteItem,
  type PaletteRequest,
  type PaletteProviderDef,
  type PerspectiveDef,
  type SharedScope,
  type SharedStateDef,
  type AssistantContextValue,
  type AssistantContextDef,
} from './src/define';
export {
  VIEWS,
  ACTIVITY_ITEMS,
  EXPLORER_PROVIDERS,
  PALETTE_PROVIDERS,
  PERSPECTIVES,
  SHARED_STATE,
  ASSISTANT_CONTEXT,
  WORKBENCH_KINDS,
  PALETTE_PREFIX,
  COMMANDS_PREFIX,
  layoutProblems,
  targetProblems,
  activityTarget,
} from './src/kinds';
export { provideWorkbenchContributions, provideWorkbenchKinds, provideLiveContributions, type WorkbenchBatch } from './src/provide';
export {
  WorkbenchRegistry,
  VIEW_PANEL,
  INSTANCE_SEPARATOR,
  MAX_INSTANCE_KEY,
  panelId,
  parsePanelId,
  isOffered,
  type PanelRef,
} from './src/registry';
export {
  WORKBENCH_PORTS,
  defaultWorkbenchPorts,
  PANEL_WINDOWS,
  DOCK_PORT,
  EXPLORER_PLAN,
  type WorkbenchPorts,
  type ShowViewOptions,
  type PanelWindowsPort,
  type PopOutPlacement,
  type DockPort,
  type ExplorerPlan,
  type ExplorerVerdict,
} from './src/ports';
