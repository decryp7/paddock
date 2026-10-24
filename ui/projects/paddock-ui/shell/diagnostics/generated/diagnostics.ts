// Generated from the Rust wire types of paddock-diag (ts-rs). Do not edit: PADDOCK_WRITE_TS=1 cargo test -p paddock-diag --all-features --lib generated.

export type FileKind = "manifest" | "description" | "system-info" | "settings" | "log" | "crash" | "app" | "other";

export type AppInfo = { 
/**
 * The app's id (`spotter`).
 */
id: string, 
/**
 * Its display name.
 */
name: string, 
/**
 * Its version.
 */
version: string, 
/**
 * The build commit, or empty.
 */
commit: string, };

export type RedactionCounts = { 
/**
 * Secrets: tokens, passwords, keys, credentials in URLs, card numbers.
 */
secrets: number, 
/**
 * Properties of log lines and settings the log's rules masked or left out (secret keys, file contents).
 */
fields: number, 
/**
 * Home folders replaced by `~`.
 */
homePaths: number, 
/**
 * Other profile paths (`/Users/x/`, `C:\Users\x\`) whose owner is not known.
 */
profilePaths: number, 
/**
 * The machine's name.
 */
machineNames: number, 
/**
 * User names.
 */
userNames: number, 
/**
 * E-mail addresses.
 */
emails: number, 
/**
 * IPv4 addresses.
 */
ipAddresses: number, };

export type PreviewFile = { 
/**
 * Path inside the zip.
 */
name: string, 
/**
 * What it is.
 */
kind: FileKind, 
/**
 * Size in bytes (uncompressed).
 */
bytes: number, 
/**
 * What it holds, in words.
 */
summary: string, };

export type Preview = { 
/**
 * The files, in zip order (the manifest first).
 */
files: Array<PreviewFile>, 
/**
 * Total bytes before compression.
 */
totalBytes: number, 
/**
 * What redaction removed, by kind.
 */
redactions: RedactionCounts, 
/**
 * What could not be collected.
 */
notes: Array<string>, };

export type Saved = { 
/**
 * Where the zip was written.
 */
path: string, 
/**
 * Its size on disk, bytes.
 */
bytes: number, 
/**
 * How many files it holds (the manifest included).
 */
files: number, };

export type PreviewReply = { 
/**
 * Identifies the collected report; pass it back to `diagnostics_save` and to later previews of the same report.
 */
token: string, 
/**
 * What the zip would hold.
 */
preview: Preview, };

export type SaveOutcome = { "state": "saved" } & Saved | { "state": "dismissed" } | { "state": "cancelled" } | { "state": "failed", 
/**
 * Words for the user.
 */
message: string, };

export type DiagErrorCode = "notSetUp" | "expired" | "busy" | "noPicker" | "failed";

export type DiagCommandError = { 
/**
 * What kind of refusal.
 */
code: DiagErrorCode, 
/**
 * Words for the user.
 */
message: string, 
/**
 * Whether trying again can help.
 */
retryable: boolean, };

/** The event the outcome of a save is sent on, to every window. */
export const DIAGNOSTICS_EVENT = 'paddock://diagnostics';
