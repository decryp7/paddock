// Generated from the Rust wire types of paddock-commands (ts-rs). Do not edit: PADDOCK_WRITE_TS=1 cargo test -p paddock-commands --lib generated.

export type CommandKind = "query" | "mutation" | "quiet";

export type WindowClass = "main" | "popout" | "dialog";

export type CommandInfo = { kind: CommandKind, windows: Array<WindowClass>, limit: number, };

export type RefusalCode = "denied" | "limit";

export type CommandRefusal = { code: RefusalCode, 
/**
 * For the user, as is. A UI recognises a denial by `code` (the wording "`<command>` is not available in " is kept
 * stable for UIs that match on it).
 */
message: string, 
/**
 * Always false: the same call is refused again.
 */
retryable: boolean, 
/**
 * The refused command.
 */
command: string, 
/**
 * The dialog whose window called it, when the caller is a dialog the dialog layer knows.
 */
dialog?: string, };
