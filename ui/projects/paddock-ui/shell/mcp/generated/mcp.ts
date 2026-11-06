// Generated from the Rust wire types of paddock-mcp (ts-rs). Do not edit: PADDOCK_WRITE_TS=1 cargo test -p paddock-mcp --lib generated.

export type Outcome = "ok" | "tool-error" | "refused" | "timed-out" | "panicked" | "cancelled";

export type McpSettings = { 
/**
 * The master switch.
 */
enabled: boolean, 
/**
 * Whether resources are exposed.
 */
resources: boolean, 
/**
 * Read-only tools the user switched off.
 */
disabledTools: Array<string>, 
/**
 * Action tools the user allowed, by name.
 */
allowedActions: Array<string>, };

export type ToolInfo = { 
/**
 * Its name.
 */
name: string, 
/**
 * A name for people.
 */
title: string | null, 
/**
 * What it does.
 */
description: string, 
/**
 * `true` for an action tool (it changes something and needs its own permission).
 */
action: boolean, 
/**
 * Whether clients see it under the saved settings.
 */
exposed: boolean, };

export type KeyInfo = { 
/**
 * The id to revoke by.
 */
id: number, 
/**
 * The client it was issued to.
 */
client: string, 
/**
 * When it was issued (epoch milliseconds).
 */
createdMs: number, 
/**
 * Revoked keys stay in the list but no longer work.
 */
revoked: boolean, };

export type ListenerView = { "kind": "unavailable" } | { "kind": "off" } | { "kind": "noKey" } | { "kind": "listening", 
/**
 * The loopback port.
 */
port: number, 
/**
 * The discovery file a client reads to find the port.
 */
discoveryFile: string, } | { "kind": "failed", 
/**
 * The cause and the next step.
 */
message: string, };

export type McpView = { 
/**
 * The saved settings.
 */
settings: McpSettings, 
/**
 * Every tool the app registered.
 */
tools: Array<ToolInfo>, 
/**
 * The clients' keys.
 */
keys: Array<KeyInfo>, 
/**
 * Most keys that can be issued.
 */
maxKeys: number, 
/**
 * Whether and where the loopback listener serves.
 */
listener: ListenerView, 
/**
 * Why the saved keys are not in use (the key file was damaged, unreadable, from a newer version or writable by others), with the
 * next step; `None` when the list is fine. While it is set every client is refused.
 */
keysProblem: string | null, };

export type IssuedKey = { 
/**
 * The key's id (in the list).
 */
id: number, 
/**
 * The client it belongs to.
 */
client: string, 
/**
 * The key. The engine keeps only its hash; it cannot be shown again.
 */
key: string, };

export type AuditRow = { 
/**
 * When (epoch milliseconds).
 */
atMs: number, 
/**
 * Who asked.
 */
client: string, 
/**
 * The tool.
 */
tool: string, 
/**
 * The arguments, redacted.
 */
arguments: unknown, 
/**
 * Bytes of the result.
 */
resultBytes: number, 
/**
 * How long it took.
 */
durationMs: number, 
/**
 * How it ended.
 */
outcome: Outcome, };

export type McpErrorCode = "invalid" | "storeUnavailable" | "failed";

export type McpError = { 
/**
 * The kind.
 */
code: McpErrorCode, 
/**
 * What to do, for the user.
 */
message: string, 
/**
 * Whether trying again can help.
 */
retryable: boolean, };

export type McpEvent = { "kind": "audit", 
/**
 * Entries now in the log.
 */
count: number, };

/** The event every audit change is sent on, to every window. */
export const MCP_EVENT = 'paddock://mcp';

/** The most client keys one app keeps. */
export const MAX_KEYS = 16;
