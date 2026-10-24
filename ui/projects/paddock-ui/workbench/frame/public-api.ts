/**
 * `@decryp7/paddock-ui/workbench/frame`: the window of an IDE-style app (ADR 0013 §1; plan W4). `provideShellWorkbench`
 * and `<pd-workbench>`: title bar and menu bar, activity bar (`pd-activity-bar`, contributed items with badges, fly-outs
 * and More), the Explorer side bar (opened and sized by the user, closed and reopened by the layout plan), the stage the
 * app fills with the dock (`[slot=stage]`), the status bar, the command palette, notifications and dialogs; F6 cycles
 * the regions. The window's `WORKBENCH_PORTS` combine the dock's `DOCK_PORT` with the frame's Explorer and palette.
 */
export { PdWorkbench } from './src/frame';
export { PdActivityBar, FLYOUT_POSITIONS } from './src/activity-bar';
export { ActivityItems, fittingTopItems, badgeText, ACTIVITY_BUTTON_PX, ACTIVITY_GAP_PX, ACTIVITY_PADDING_PX } from './src/activity';
export {
  WorkbenchFrameStore,
  WORKBENCH_FRAME_STORAGE,
  WORKBENCH_FRAME_KEY,
  WORKBENCH_FRAME_VERSION,
  EXPLORER_LAYOUT,
  STAGE_LAYOUT,
  ACTIVITY_BAR_PX,
  EXPLORER_MARGIN_PX,
  EXPLORER_MIN_PX,
  EXPLORER_MAX_PX,
  fallbackVerdict,
  loadFramePrefs,
  saveFramePrefs,
  clampWidth,
  type FramePrefs,
  type FrameStorage,
  type WorkbenchFrameStoreType,
} from './src/frame.store';
export { provideShellWorkbench, workbenchPorts, WORKBENCH_CONFIG, FRAME_COMMANDS, JOBS_VIEW, type ShellWorkbenchOptions, type WorkbenchConfig } from './src/provide-workbench';
