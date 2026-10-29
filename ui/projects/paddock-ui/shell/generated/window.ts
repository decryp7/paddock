// Generated from the Rust wire types of paddock-window (ts-rs). Do not edit: PADDOCK_WRITE_TS=1 cargo test -p paddock-window --lib generated.

export type WindowInfo = { label: string, title: string, focused: boolean, visible: boolean, };

export type OpenAsk = { count: number, names: Array<string>, };

export type WindowErrorCode = "notReady" | "closed" | "tooMany" | "notAllowed" | "failed" | "insidePluginHook";

export type WindowError = { code: WindowErrorCode, 
/**
 * For the user, as is.
 */
message: string, };

export type DeepLink = { 
/**
 * The link as parsed (normalised).
 */
url: string, 
/**
 * The scheme, lowercase (one of the app's).
 */
scheme: string, 
/**
 * What the link points at: the host part (`models`), possibly empty.
 */
host: string, 
/**
 * The path after the host (`/llama`), possibly empty.
 */
path: string, 
/**
 * The query parameters, decoded, in order.
 */
query: Array<[string, string]>, };

export type OpenRequest = { 
/**
 * Canonical paths of existing files and folders.
 */
paths: Array<string>, links: Array<DeepLink>, };

export type OnClose = "quit" | "tray" | "ask";

export type CloseChoice = "quit" | "tray";

export type OpenChoice = "here" | "newWindow" | "cancel";

/** The events of the window layer: to every window (`windows`) and to one window (the others). */
export const WINDOW_EVENTS = {
  windows: 'paddock://windows',
  closeRequested: 'paddock://window-close-requested',
  openAsk: 'paddock://window-open-ask',
  open: 'paddock://window-open',
} as const;
