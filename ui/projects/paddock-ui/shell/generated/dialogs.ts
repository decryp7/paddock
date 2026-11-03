// Generated from the Rust wire types of paddock-dialogs (ts-rs). Do not edit: PADDOCK_WRITE_TS=1 cargo test -p paddock-dialogs --lib generated.

export type DialogKind = "decision" | "form" | "viewer" | "tool";

export type DialogModality = "modal" | "modeless";

export type DialogSize = { width: number, height: number, };

export type DialogMeasureWire = { content: DialogSize, minContent: DialogSize, 
/**
 * The page's `window.innerHeight` (CSS px), when the page sends it. A webview can be shorter than the window's
 * inner height (the 1.1-0 spike measured 32 px on a Mac whose outer and inner sizes were equal): the fit adds the
 * difference ([`viewport_delta`]) to the content and minimum content heights so the footer is not cut off.
 */
viewportHeight?: number, 
/**
 * The page's `window.devicePixelRatio` when it measured. A webview zoomed away from the window's scale factor
 * (browser zoom, an OS text scale) reports CSS px that are not logical px: the fit converts with [`zoom_of`].
 * Absent: CSS px are taken as logical.
 */
devicePixelRatio?: number, 
/**
 * The body scrolls (`scrollHeight > clientHeight + 1`): logged, so a scrollbar beside one line is seen in a user's
 * bundle; never used to size (ADR 0043 §2).
 */
overflow: boolean, };

export type DialogRemembered = { dx: number, dy: number, width: number, height: number, };

export type DialogTheme = "light" | "dark";

export type DialogRequest = { dialogId: string, 
/**
 * Tells apart the windows of a dialog that is open once per input (a hash of the input; letters and digits).
 */
instance?: string, title: string, 
/**
 * The typed input of the dialog, handed to its webview through `dialog_take_input`.
 */
input?: unknown, 
/**
 * What the UI remembered of this dialog: its size and its offset from the parent window (never an absolute
 * screen position). The shell decides where it goes ([`placement::place`]).
 */
remembered?: DialogRemembered, 
/**
 * The opener's resolved theme (its `data-theme`): the window's background before the page paints is that theme's
 * `--pd-surface-2`. Without it the parent window's OS theme is used.
 */
theme?: DialogTheme, };

export type DialogInfo = { label: string, dialogId: string, 
/**
 * The window existed already (and was focused, if it was on screen).
 */
existing: boolean, 
/**
 * The window is not on screen yet: it is shown when its page is ready (or at the show deadline), and
 * `paddock://dialog-shown` follows. False for an existing window that was shown already.
 */
pending: boolean, };

export type DialogStart = { dialogId: string, title: string, kind: DialogKind, modality: DialogModality, 
/**
 * No Cancel: Escape and a close request leave it open.
 */
requiredChoice: boolean, input: unknown, 
/**
 * This page load of the window (1 for the first, raised by every load, also a reload): every measure the page sends
 * carries it, so a reloaded page's `seq = 1` is not dropped as stale (ADR 0043 §2).
 */
epoch: number, };

export type DialogReadyWire = { epoch: number, seq: number, };

export type DialogSizeModeName = "manual" | "height";

export type DialogSizeModeEvent = { mode: DialogSizeModeName, };

export type DialogRemeasureEvent = { 
/**
 * The window's new scale factor.
 */
scale: number, };

export type DialogResult = { label: string, dialogId: string, result: unknown, };

export type DialogShown = { label: string, dialogId: string, };

export type DialogClosed = { label: string, dialogId: string, 
/**
 * Its size and its offset from the parent window when it closed (logical px), for remembering.
 */
remembered: DialogRemembered | null, };

export type DialogErrorCode = "invalid" | "unknown" | "limit" | "notADialog" | "closed" | "notReady" | "failed" | "insidePluginHook";

export type DialogError = { code: DialogErrorCode, 
/**
 * For the user, as is.
 */
message: string, 
/**
 * Whether the same call may succeed later (the layer was not ready, the window system refused).
 */
retryable: boolean, };

/** The events of the dialog layer: to the opener (`result`, `shown`, `closed`) and to the dialog (`closeRequested`, `inputs`, `sizeMode`, `remeasure`). */
export const DIALOG_EVENTS = {
  result: 'paddock://dialog-result',
  shown: 'paddock://dialog-shown',
  closed: 'paddock://dialog-closed',
  closeRequested: 'paddock://dialog-close-requested',
  inputs: 'paddock://dialog-inputs',
  sizeMode: 'paddock://dialog-size-mode',
  remeasure: 'paddock://dialog-remeasure',
} as const;

/** The commands every dialog window may call whatever its contract says. */
export const DIALOG_BASE_COMMANDS = ['dialog_take_input', 'dialog_close', 'dialog_ready', 'dialog_fit', 'notifications_post', 'notifications_list'] as const;
