// Generated from the Rust wire types of paddock-commands (ts-rs). Do not edit: PADDOCK_WRITE_TS=1 cargo test -p paddock-commands --lib generated.

export type ErrorCode = "failed" | "invalid" | "notFound" | "notGranted" | "busy" | "cancelled" | "io" | "limit" | "unsupported" | "unavailable" | "denied" | "missing" | "timeout" | "internal";

export type CommandError = { 
/**
 * What kind of failure.
 */
code: ErrorCode, 
/**
 * For the user, as is: what happened.
 */
message: string, 
/**
 * Whether the same call can succeed later (the UI offers Retry).
 */
retryable: boolean, 
/**
 * The next step for the user.
 */
hint?: string, 
/**
 * The argument a refusal is about.
 */
field?: string, 
/**
 * The service outage this is a symptom of (`health:store`): service health reports it once and no panel repeats it.
 */
cause?: string, 
/**
 * The app's own distinction within the code (`noModel`, `unknownRunner`); the UI of that app matches on it.
 */
kind?: string, };

/** Every error code, for validating what arrives over the wire. */
export const ERROR_CODES: readonly ErrorCode[] = ['failed', 'invalid', 'notFound', 'notGranted', 'busy', 'cancelled', 'io', 'limit', 'unsupported', 'unavailable', 'denied', 'missing', 'timeout', 'internal'];
