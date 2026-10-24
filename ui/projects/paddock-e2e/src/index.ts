/**
 * `@decryp7/paddock-e2e`: the Playwright side of the Paddock shell test kit (ADR 0012 §7, plan S16): `test` and
 * `expect` with the Chrome for Testing guard (never the installed Chrome, rule 7) and the kit's fixtures; the e2e port
 * from the environment; axe in both themes and over every dialog window; keyboard-only flow helpers (Tab walks, reachability, focus return); page objects for the frame, sidebar, status bar, notifications, jobs,
 * menus, dialogs and Settings; layout scenarios; the fake Tauri IPC; screenshots of a settled frame; the app version from its single source; and the
 * retried-test reporter (`@decryp7/paddock-e2e/retried-reporter`).
 */
export { test, expect, type PaddockFixtures } from './fixtures.ts';
export type { Page, Locator, BrowserContext } from '@playwright/test';
export { e2ePort } from './env.ts';
export { assertChromeForTesting, browserProblems, isInstalledBrowser, CHROME_ENV, INSTALLED_CHANNELS, type BrowserUse, type GuardedConfig } from './browser.ts';
export { axeViolations, axeInThemes, axeTargets, WCAG_TAGS, THEMES, type Theme, type AxeOptions, type AxeTarget } from './axe.ts';
export { axeEveryDialog, dialogTargets, type EveryDialogOptions } from './a11y.ts';
export {
  tabWalk,
  walkStep,
  focusStop,
  expectTabOrder,
  expectReachableByKeyboard,
  unreachableByKeyboard,
  expectEscapeReturnsFocus,
  type FocusStop,
  type TabWalk,
  type TabWalkOptions,
  type WalkOutcome,
} from './keyboard.ts';
export { LAYOUT_WIDTHS, WINDOW_SIZES, horizontalOverflow, expectNoHorizontalScroll, expectWithinWidth, resize } from './layout.ts';
export { waitForStableFrame, captureStable, type StableFrameOptions } from './screenshot.ts';
export { installTauriFake, TauriFake, START_REPLIES, SHELL_PLUGIN_PREFIX, type IpcCall, type TauriFakeOptions } from './ipc.ts';
export {
  ShellApp,
  ShellFrame,
  ShellSidebar,
  ShellStatusBar,
  ShellNotifications,
  ShellJobs,
  ShellMenus,
  ShellDialogs,
  ShellSettings,
  dialogContracts,
  dialogWindowUrl,
  DIALOG_RESULT_KEY,
  CLOSE_NAMES,
  type DialogContractEntry,
} from './shell.ts';
export { renderFlaky, type Flaky } from './retried-reporter.ts';
export { readVersion, RELEASE_VERSION } from './version.ts';
