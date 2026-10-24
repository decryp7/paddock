/**
 * `@decryp7/paddock-ui/workbench/dock`: the dock (ADR 0013; plan W2a, W2b). `<pd-dock />` hosts dockview and fills its
 * container; `WorkbenchDock` (from `provideWorkbenchDock`) opens, closes and arranges panels for the registered views,
 * plans them with `planLayout` (ADR 0011: nothing below its minimum, the lowest priority tabbed or auto-hidden first,
 * grow-back with hysteresis), keeps perspectives, named layouts and the saved arrangement through a `DockLayoutStore`.
 * Needs the optional peer `dockview` and its stylesheet (`dockview/dist/styles/dockview.css`); dockview loads on the
 * first `<pd-dock />`, so windows without a dock never bundle it.
 */
export { provideWorkbenchDock } from './src/provide';
export { PdDock } from './src/dock-host';
export { WorkbenchDock } from './src/dock.service';
export { type DockOptions } from './src/dock-options';
export {
  type DockLayoutStore,
  InMemoryDockLayoutStore,
  BrowserDockLayoutStore,
  IpcDockLayoutStore,
  DOCK_LAYOUT_STORE,
  LAYOUT_KEY_PATTERN,
  LAYOUT_VALUE_MAX,
  LAYOUT_GET,
  LAYOUT_SET,
  layoutEntryProblem,
  type DockInvoke,
} from './src/dock-store';
export { dockContributions, provideDockContributions, DOCK_COMMANDS } from './src/dock-contributions';
export { PANEL_VISIBLE } from './src/angular-panel';
export { type DockNotice } from './src/dock-context';
export { Perspectives, PERSPECTIVE_KEYS, type PerspectiveInfo } from './src/perspectives';
export { WindowLayouts } from './src/window-layouts';
export { type NamedLayout } from './src/named-layouts';
export { type SavedLayout, type DockExtras, readLayout, toEnvelope, ENVELOPE_VERSION } from './src/layout-store';
export { type DockSlot, type Side, type Point, type Rect } from './src/dock-geometry';
export { type ViewKind } from './src/view-kind';
export { type SwitcherState } from './src/switcher';
export { type AutoHideState, type AutoHideItem } from './src/auto-hide';
