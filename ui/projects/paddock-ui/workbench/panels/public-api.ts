/**
 * `@decryp7/paddock-ui/workbench/panels`: pop-out panel windows and multi-window state over `paddock-panels` (plan X50 W3).
 *
 * - Host: `providePanelWindows(io)` provides `PanelWindows` for the core's `PANEL_WINDOWS` port (pop a panel out, learn when its
 *   window closed, the drag ghost for a tab dragged out, the cursor of a pop-out dragged back over the window).
 * - Pop-out window: `providePopoutWindow(io)` and `PdPopoutShell` (title bar with Dock back, the one view with only its own
 *   providers), `parsePopoutRoute` to decide at boot which window this is.
 * - Shared state: the registry's `SharedStateDef`s synced between a host, its pop-outs and (display scope) its dialogs by
 *   `UiSync`; Rust forwards only within one host, bounded and rate-limited.
 * - Window systems: `tauriPanelsIo(bridge)` and `tauriWindowChrome(window, mac)` for the desktop build, `InPageDesktop` for
 *   the browser build and specs (simulated windows, no OS window opens).
 */
export { providePanelWindows, providePopoutWindow, provideDialogUiSync, mountPopoutWindowHost, type PopoutWindowOptions } from './src/provide';
export { PanelWindows, PANEL_WINDOWS_OPTIONS, HOST_TITLE_KEY, hostTitleValue, parseDragOver, type PanelWindowsOptions, type PopOutMoved } from './src/panel-windows';
export { PdPopoutShell, pinDocumentTitle, SYNC_START_TIMEOUT_MS, POPOUT_APP_NAME } from './src/popout-shell';
export { PdPopoutTitleBar } from './src/popout-title-bar';
export { PopoutState } from './src/popout-state';
export { POPOUT_ROUTE, POPOUT_ROUTE_PATH, parsePopoutRoute, popoutHash, type PopoutRoute } from './src/popout-route';
export { windowTitle, MAX_TITLE } from './src/window-title';
export { PANELS_IO, tauriPanelsIo, parsePanelsEvent, type PanelsIo, type PanelsBridge, type GhostIo, type Unlisten } from './src/io';
export { WINDOW_CHROME, tauriWindowChrome, browserWindowChrome, type WindowChromeIo, type TauriWindowBridge, type ChromeKind, type ResizeDirection } from './src/chrome';
export { InPageDesktop, InPageWindow, MockGhost, NO_GHOST, recipients, hostOf, validPanel, type MockGhostState } from './src/in-page';
export { GhostSession, type DragOutPreview } from './src/ghost-session';
export { snapshotPanel, SNAPSHOT_MAX_BYTES, SNAPSHOT_MAX_NODES, SNAPSHOT_TIMEOUT_MS, type Size } from './src/panel-snapshot';
export { UiSync, syncedValues, SYNC_TIMERS, HELLO_INTERVAL_MS, HELLO_TRIES, type SyncedValue, type SyncRole, type SyncTimers, type HostCommandHandler } from './src/ui-sync';
export { parseSyncMessage, SyncEngine, newOrigin, fingerprint, MAX_MESSAGE_CHARS, type SyncMessage } from './src/sync-protocol';
export * from './generated/panels';
export { PANEL_COMMANDS, type PanelCommand } from './generated/panel-commands';
