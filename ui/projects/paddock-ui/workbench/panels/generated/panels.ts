// Generated from the Rust wire types of paddock-panels (ts-rs). Do not edit: PADDOCK_WRITE_TS=1 cargo test -p paddock-panels --lib generated.

export type PopOutBounds = { x: number, y: number, width: number, height: number, 
/**
 * The scale factor of the display `x` and `y` are logical px of.
 */
scale?: number, };

export type PopOutRequest = { 
/**
 * The panel id (`pn.code` or `pn.code#key`).
 */
panel: string, 
/**
 * The panel title (the window title is "<title> - <host title>").
 */
title?: string, 
/**
 * Where to open; without it the place remembered for the view, else beside the host window.
 */
bounds?: PopOutBounds, 
/**
 * Stay above the host window (an owned window), not above everything.
 */
keepOnTop?: boolean, };

export type PopOutInfo = { label: string, host: string, panel: string, title: string, 
/**
 * Where it is now (logical px), when the platform says.
 */
bounds: PopOutBounds | null, 
/**
 * The name of the display it is on, when the platform says.
 */
monitor: string | null, keepOnTop: boolean, };

export type PanelsEvent = { "kind": "returned", panel: string, label: string, } | { "kind": "changed" } | { "kind": "moved", panel: string, label: string, x: number, y: number, };

export type SyncScope = "full" | "display";

export type SyncPublish = { scope: SyncScope, 
/**
 * The message (a JSON object; the receivers parse it again).
 */
message: unknown, };

export type GhostRequest = { 
/**
 * The panel's name, shown in the ghost's title bar.
 */
title: string, 
/**
 * The panel's icon: the `d` of an SVG path on a 24x24 grid.
 */
icon?: string, 
/**
 * A snapshot of the panel taken when the drag started: `data:image/png;base64,...`.
 */
snapshot?: string, 
/**
 * The window's colour scheme is dark (the ghost matches it).
 */
dark?: boolean, 
/**
 * The size the popped-out window will get (logical px).
 */
width: number, height: number, 
/**
 * Where the pointer holds the ghost, from its top-left corner (logical px).
 */
grabX: number, grabY: number, };

export type PanelErrorCode = "notReady" | "closed" | "tooMany" | "invalid" | "tooLarge" | "rateLimited" | "notAllowed" | "failed";

export type PanelError = { code: PanelErrorCode, 
/**
 * For the user, as is.
 */
message: string, };

/** The events of the panel layer: to the host window only (`panels`) and to the other windows of a host (`uiSync`). */
export const PANEL_EVENTS = {
  panels: 'paddock://panels',
  uiSync: 'paddock://ui-sync',
} as const;

/** The most pop-outs one window can have, the largest forwarded UI message (bytes) and the longest instance key. */
export const PANEL_LIMITS = {
  maxPopOuts: 12,
  maxMessageBytes: 524288,
  maxKey: 1024,
} as const;
