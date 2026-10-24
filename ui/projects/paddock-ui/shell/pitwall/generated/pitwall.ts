// Generated from the Rust wire types of paddock-pitwall (ts-rs). Do not edit: PADDOCK_WRITE_TS=1 cargo test -p paddock-pitwall --all-features --lib generated.

/** Every window hears it when the app's Pitwall snapshot changed; the payload is an `AiSnapshot`. */
export const PITWALL_EVENT = 'paddock://pitwall';

/** The commands the app registers for the shared component (snapshot, connect, cancel pairing, use the built-in engine). */
export const PITWALL_COMMANDS = ['pitwall_state', 'pitwall_connect', 'pitwall_cancel_pairing', 'pitwall_use_builtin'] as const;

export type PitwallCommand = (typeof PITWALL_COMMANDS)[number];

export type NextStep = "startPitwall" | "installPitwall" | "pair" | "installModel" | "wait" | "updateApp" | "updatePitwall" | "changePermissions" | "none";

export type Caps = { 
/**
 * Chat completions.
 */
chat: boolean, 
/**
 * Images in the prompt.
 */
vision: boolean, 
/**
 * Native tool calls.
 */
tools: boolean, 
/**
 * JSON-schema answers.
 */
json: boolean, 
/**
 * Embeddings.
 */
embeddings: boolean, 
/**
 * At least this many tokens of context.
 */
contextTokens: number | null, 
/**
 * Prefer the fastest model.
 */
fast: boolean, };

export type AiState = { "kind": "disabled" } | { "kind": "notInstalled" } | { "kind": "notRunning" } | { "kind": "incompatible", 
/**
 * Pitwall's reason, with whom to update.
 */
message: string, 
/**
 * The app is the older side.
 */
updateApp: boolean, } | { "kind": "needsPairing" } | { "kind": "paused", 
/**
 * Stopped rather than paused.
 */
stopped: boolean, } | { "kind": "noSuitableModel", 
/**
 * What the app asked for.
 */
needs: Caps, } | { "kind": "unavailable", 
/**
 * The reason, one sentence.
 */
message: string, } | { "kind": "ready", 
/**
 * The model Pitwall picked.
 */
model: string, 
/**
 * It accepts images.
 */
vision: boolean, 
/**
 * The context the app can plan prompts for.
 */
contextTokens: number, };

export type NeedsAiView = { 
/**
 * The feature, as the user knows it ("Ask about this log").
 */
feature: string, 
/**
 * Why it is not available.
 */
reason: AiState, 
/**
 * The sentence to show.
 */
message: string, 
/**
 * The buttons, best first.
 */
actions: Array<NextStep>, };

export type PairingView = { 
/**
 * The six-digit code the user compares in Pitwall.
 */
code: string, 
/**
 * When the request expires (Unix ms).
 */
expiresAtMs: number, };

export type AiSnapshot = { 
/**
 * What the app can do with Pitwall now.
 */
state: AiState, 
/**
 * The app's name.
 */
app: string, 
/**
 * One plain sentence for the user (the engine's wording, naming `app`).
 */
message: string, 
/**
 * The buttons that fix it, best first; empty when nothing needs fixing.
 */
actions: Array<NextStep>, 
/**
 * The pairing code while one waits for the user's approval in Pitwall.
 */
pairing: PairingView | null, 
/**
 * The app has an engine of its own, so "Use the built-in engine" is a working repair (rule 19).
 */
builtInAvailable: boolean, 
/**
 * The user already switched this session to the app's own engine.
 */
builtInForSession: boolean, };
