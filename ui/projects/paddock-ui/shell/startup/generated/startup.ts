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

export type Mode = "normal" | "safe";

export type SessionInfo = { 
/**
 * There is a session to restore.
 */
available: boolean, 
/**
 * How many windows it has.
 */
windows: number, 
/**
 * Why the session could not be used as it was (damaged, from a newer version), in a sentence for the notice.
 */
problem: string | null, };

export type CrashInfo = { 
/**
 * When (RFC 3339, UTC).
 */
time: string, 
/**
 * The panicking thread.
 */
thread: string, 
/**
 * `file:line` of the panic.
 */
location: string, 
/**
 * The crash happened in safe mode.
 */
inSafeMode: boolean, };

export type Offers = { 
/**
 * Reopen the session of the run that ended badly.
 */
restore: boolean, 
/**
 * Start again with extensions, plugins and the last session off.
 */
safeMode: boolean, 
/**
 * Forget the session and start clean.
 */
startFresh: boolean, };

export type UncleanShutdown = { 
/**
 * The panic, when one was recorded.
 */
crash: CrashInfo | null, 
/**
 * The session that could be restored.
 */
session: SessionInfo, 
/**
 * What the notice offers.
 */
offers: Offers, };

export type StartReport = { 
/**
 * This run is in safe mode (the title bar shows it).
 */
safeMode: boolean, 
/**
 * Set after an unclean shutdown until the user decides.
 */
unclean: UncleanShutdown | null, };

export type RestorePlan = { 
/**
 * The windows of the session, in label order (the app reopens the ones it knows).
 */
windows: Array<string>, 
/**
 * The asking window's own UI state, if the session had one.
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
