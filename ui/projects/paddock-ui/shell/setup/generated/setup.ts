// Generated from the Rust wire types of paddock-shell's set-up checklist (ts-rs). Do not edit: PADDOCK_WRITE_TS=1 cargo test -p paddock-shell --lib setup::generated.

export type ItemStatus = "done" | "todo" | "problem";

export type ItemView = { 
/**
 * The item's id.
 */
id: string, 
/**
 * The step's name.
 */
title: string, 
/**
 * What it does and why.
 */
detail: string, 
/**
 * The primary button's label.
 */
action: string, 
/**
 * Where it stands.
 */
status: ItemStatus, 
/**
 * What is wrong, or why the last action did not work.
 */
message: string | null, 
/**
 * The user chose to skip it for now (it stays in Settings).
 */
skipped: boolean, 
/**
 * An action runs for it now.
 */
working: boolean, 
/**
 * What to do next while it waits (a code to compare in another app).
 */
hint: string | null, 
/**
 * The shared needs-AI view (Pitwall's step only).
 */
needsAi: import('@decryp7/paddock-ui/shell/pitwall').NeedsAiView | null, };

export type SetupState = { 
/**
 * The steps, in order.
 */
items: Array<ItemView>, 
/**
 * The checklist is finished or dismissed: it does not open by itself any more (Settings still has it).
 */
finished: boolean, 
/**
 * The window opens the checklist by itself now: not finished and a step is open (not done, not skipped).
 */
showOnStart: boolean, };

export type SetupErrorCode = "notSetUp" | "unknown" | "busy" | "failed" | "notSaved";

export type SetupCommandError = { 
/**
 * The kind.
 */
code: SetupErrorCode, 
/**
 * A sentence that says what to do.
 */
message: string, 
/**
 * Trying again can help.
 */
retryable: boolean, };

/** The event every change of the checklist is sent on, to every window (a `SetupState`). */
export const SETUP_EVENT = 'paddock://setup';

/** The id of the Set-up settings page. */
export const SETUP_PAGE_ID = 'pd.setup';
