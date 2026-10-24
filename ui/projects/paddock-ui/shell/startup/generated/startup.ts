// Generated from the Rust wire types of paddock-startup (ts-rs). Do not edit: PADDOCK_WRITE_TS=1 cargo test -p paddock-startup --lib generated.

export type Recovery = "unresponsive" | "hung" | "terminated";

export type ReadyReply = { 
/**
 * Set when the shell reloaded this window before this page loaded.
 */
recovered: Recovery | null, 
/**
 * With `recovered`: the window's last UI state checkpoint for the reloaded page to apply. The engine kept the data;
 * this is the rest (query, filters, selection).
 */
ui: unknown, };

export type BootErrorKind = "error" | "rejection" | "bootstrap" | "other";

export type PingEvent = { 
/**
 * The ping's number; `window_pong` answers with it.
 */
seq: number, };

/** The event a ready window gets as a liveness ping; it answers with `window_pong` and the same `seq`. */
export const PING_EVENT = 'paddock://ping';
