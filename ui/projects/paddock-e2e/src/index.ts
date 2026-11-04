/**
 * `@decryp7/paddock-e2e`: the Playwright side of the Paddock shell test kit (ADR 0012 §7, plan S16): `test` and
 * `expect` with the Chrome for Testing guard (never the installed Chrome, rule 7) and the kit's fixtures; the e2e port
 * from the environment and the config preset (`paddockConfig`); the documentation screenshot runner (`captureDocScreens`); axe in both themes and over every dialog window; keyboard-only flow helpers (Tab walks, reachability, focus return); page objects for the frame, sidebar, status bar, notifications, jobs,
 * menus, dialogs and Settings; layout scenarios; the fake Tauri IPC; screenshots of a settled frame; the app version from its single source; and the
 * retried-test reporter (`@decryp7/paddock-e2e/retried-reporter`).
 */
export { test, expect, type PaddockFixtures } from './fixtures.ts';
export type { Page, Locator, BrowserContext } from '@playwright/test';
export { e2ePort } from './env.ts';
export { clickShape, hoverShape, pointInShape, type PagePoint } from './svg.ts';
export { assertChromeForTesting, browserProblems, isInstalledBrowser, pinnedChrome, CHROME_ENV, CHROMIUM_TEST_ENV, INSTALLED_CHANNELS, type BrowserUse, type GuardedConfig } from './browser.ts';
export { paddockConfig, isOn, SCREENSHOT_SPECS, DOC_SCREEN_VIEWPORT, type PaddockConfigOptions } from './config.ts';
export {
  captureDocScreens,
  planShots,
  recordHashes,
  sha256,
  FRAME_MAX_HEIGHT,
  FROZEN_AT,
  HASHES_FILE,
  type DocScreen,
  type DocScreensOptions,
  type PlannedShot,
} from './doc-screens.ts';
export { axeViolations, settleTransitions, axeInThemes, axeTargets, WCAG_TAGS, THEMES, type Theme, type AxeOptions, type AxeTarget } from './axe.ts';
export { axeEveryDialog, dialogTargets, type EveryDialogOptions } from './a11y.ts';
export {
  tabWalk,
  walkStep,
  focusStop,
  tabKey,
  pageTabKey,
  pressTab,
  bindingFor,
  modKey,
  pageModKey,
  pressHostMod,
  pressMod,
  expectTabOrder,
  expectReachableByKeyboard,
  unreachableByKeyboard,
  expectEscapeReturnsFocus,
  type FocusStop,
  type NavigatorPlatform,
  type TabWalk,
  type TabWalkOptions,
  type WalkOutcome,
} from './keyboard.ts';
export { LAYOUT_WIDTHS, WINDOW_SIZES, horizontalOverflow, expectNoHorizontalScroll, expectWithinWidth, resize, isSilentlyClipped, isAcceptedEllipsis, unacceptedTextClips, ELLIPSIS_NEVER_WITHIN, ELLIPSIS_NEVER_SELF, textClippedWithoutEllipsis, textCutOffWithoutTooltip, type TextClip, expectNoSilentClipping, type ClipProbe } from './layout.ts';
export { headerCovers, headerTrack, expectHeaderOnTop, isOpaqueColor, type GridParts, type HeaderCover, type HeaderFrame, type HeaderReport } from './grid.ts';
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
  parseDialogContractsModule,
  dialogWindowUrl,
  DIALOG_RESULT_KEY,
  CLOSE_NAMES,
  type DialogContractEntry,
} from './shell.ts';
export { dialogFitProblems, openEveryDialog, measureDialogWindow, FIT_SCALES, FIT_WORK_AREAS, MAX_FOOTER_GAP, type DialogFitOptions, type DialogWindowMeasure } from './dialog-fits.ts';
export { WorkbenchApp, WorkbenchFrame, WorkbenchActivityBar, WorkbenchExplorer, WorkbenchPalette } from './workbench.ts';
export { renderFlaky, type Flaky } from './retried-reporter.ts';
export { readVersion, RELEASE_VERSION } from './version.ts';
export {
  appBuildGuard,
  appBuildGuardOf,
  assertFreshBuilds,
  reuseServer,
  uiConfigGuard,
  stampBuild,
  staleBuilds,
  sourceHash,
  REBUILD_HINT,
  type BuildGuard,
} from './build-guard.ts';
export { expectLoopSettles, expectSizeToContent, SIZE_SCALES, type SizeFixture } from './size-to-content.ts';
