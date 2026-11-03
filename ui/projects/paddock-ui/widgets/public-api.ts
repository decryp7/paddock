/**
 * `@decryp7/paddock-ui/widgets` (family ADR 0035): the widget area, frame and chooser every app's widget surface uses (a tray
 * popup, a dashboard, a menu-bar editor). The layout, limits, edits and undo live in the engine (`paddock-widgets`); these
 * components draw its generated types and emit `Edit` values. Only the generated types are used, never hand-written mirrors.
 */
export type * from './generated/widgets';
export { PdWidgetArea } from './src/area';
export { PdWidgetChooser } from './src/chooser';
export { PdWidgetFrame } from './src/frame';
export { ManualFrameScheduler, PD_FRAME_SCHEDULER, type FrameScheduler } from './src/frame-scheduler';
export { PD_WIDGET_GRID_BACKEND, type GridView, type PreviewReply, type WidgetGridBackend } from './src/grid-backend';
export { PdGridGesture, type GridGestureKind, type GridGestureTarget } from './src/grid-gesture';
export { PdWidgetGrid } from './src/grid';
export { relativeAgo } from './src/time';
export {
  defaultKindOf,
  PD_WIDGET_LOADING,
  PdSkeleton,
  PdWidget,
  PdWidgetOption,
  type PdWidgetContext,
  type PdWidgetOptionContext,
} from './src/widget';
export { blockText } from './src/grid-text';
