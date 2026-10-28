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
viewportHeight?: number, };

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
 * The window existed already and was focused.
 */
existing: boolean, };

export type DialogStart = { dialogId: string, title: string, kind: DialogKind, modality: DialogModality, 
/**
 * No Cancel: Escape and a close request leave it open.
 */
requiredChoice: boolean, input: unknown, };

export type DialogResult = { label: string, dialogId: string, result: unknown, };

export type DialogShown = { label: string, dialogId: string, };

export type DialogClosed = { label: string, dialogId: string, 
/**
 * Its size and its offset from the parent window when it closed (logical px), for remembering.
 */
remembered: DialogRemembered | null, };

export type DialogErrorCode = "invalid" | "unknown" | "limit" | "notADialog" | "closed" | "notReady" | "failed";

export type DialogError = { code: DialogErrorCode, 
/**
 * For the user, as is.
 */
message: string, 
/**
 * Whether the same call may succeed later (the layer was not ready, the window system refused).
 */
retryable: boolean, };

/** The events of the dialog layer: to the opener (`result`, `shown`, `closed`) and to the dialog (`closeRequested`). */
export const DIALOG_EVENTS = {
  result: 'paddock://dialog-result',
  shown: 'paddock://dialog-shown',
  closed: 'paddock://dialog-closed',
  closeRequested: 'paddock://dialog-close-requested',
} as const;

/** The commands every dialog window may call whatever its contract says. */
export const DIALOG_BASE_COMMANDS = ['dialog_take_input', 'dialog_close', 'dialog_ready', 'dialog_fit', 'notifications_post', 'notifications_list'] as const;
