// Generated from the Rust wire types of paddock-packs (ts-rs). Do not edit: PADDOCK_WRITE_TS=1 cargo test -p paddock-packs --lib generated.

export type Origin = "builtin" | "user";

export type Problem = { 
/**
 * The pack's file name (or the built-in pack's name).
 */
file: string, 
/**
 * What to fix, in words for the user.
 */
message: string, };

export type Health = { "level": "ok" } | { "level": "notes", 
/**
 * What to tell the user.
 */
message: string, } | { "level": "degraded", 
/**
 * What to tell the user (names the first problem and how many more).
 */
message: string, };

export type PackEntry = { 
/**
 * The file name (or the built-in pack's name).
 */
name: string, 
/**
 * Built-in or the user's.
 */
origin: Origin, };

export type KindView = { 
/**
 * The kind's id (`cleanup`).
 */
id: string, 
/**
 * The kind's name for the user ("Cleanup packs").
 */
label: string, 
/**
 * Counts up with every real change.
 */
revision: number, 
/**
 * The packs in use.
 */
packs: Array<PackEntry>, 
/**
 * Packs refused, with what to fix.
 */
problems: Array<Problem>, 
/**
 * Keys that were ignored.
 */
warnings: Array<Problem>, 
/**
 * The one-line summary.
 */
health: Health, };

export type PacksView = { 
/**
 * The kinds, in the order the app registered them.
 */
kinds: Array<KindView>, };

/** The event every change of a pack kind is sent on, to every window (a `PacksView`). */
export const PACKS_EVENT = 'paddock://packs';
