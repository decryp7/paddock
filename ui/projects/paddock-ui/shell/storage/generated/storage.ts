// Generated from the Rust wire types of paddock-storage (ts-rs). Do not edit: PADDOCK_WRITE_TS=1 cargo test -p paddock-storage --all-features --lib generated.

export type StorageKind = "cache" | "data";

export type ItemView = { 
/**
 * The file or folder name.
 */
name: string, 
/**
 * Its size.
 */
bytes: number, 
/**
 * Why it stays when clearing, or `None` when clearing removes it.
 */
protected?: string, };

export type StorageCategory = { 
/**
 * The category's id (what `storage_clear` names).
 */
id: string, 
/**
 * What it is called.
 */
label: string, 
/**
 * Cache or the user's data.
 */
kind: StorageKind, 
/**
 * The folder, for display and "Show in folder".
 */
path?: string, 
/**
 * Whether the folder exists (nothing to show in the file manager otherwise).
 */
exists: boolean, 
/**
 * Bytes in the category.
 */
bytes: number, 
/**
 * Files in the category.
 */
files: number, 
/**
 * What clearing would remove now (protected items left out).
 */
clearableBytes: number, 
/**
 * How many top-level items clearing would remove.
 */
clearableItems: number, 
/**
 * The walk stopped at its entry or time limit: the sizes are lower bounds ("at least").
 */
capped: boolean, 
/**
 * Whether it can be cleared.
 */
clearable: boolean, 
/**
 * Why it cannot be cleared, or what clearing keeps.
 */
note?: string, 
/**
 * The biggest items (at most [`crate::MAX_PREVIEW_ITEMS`]), biggest first.
 */
items: Array<ItemView>, };

export type StorageUsage = { 
/**
 * One row per category, in the app's order.
 */
categories: Array<StorageCategory>, 
/**
 * Bytes in all categories.
 */
totalBytes: number, 
/**
 * Any category's sizes are lower bounds.
 */
capped: boolean, };

export type Skipped = { 
/**
 * What was left.
 */
path: string, 
/**
 * Why, in words.
 */
reason: string, };

export type CategoryClearResult = { 
/**
 * The category.
 */
id: string, 
/**
 * Bytes freed.
 */
freedBytes: number, 
/**
 * Files removed.
 */
removedFiles: number, 
/**
 * The first items left in place (at most [`crate::MAX_SKIPPED_LISTED`]).
 */
skipped: Array<Skipped>, 
/**
 * All items left in place.
 */
skippedCount: number, 
/**
 * The category could not be cleared at all.
 */
error?: string, };

export type ClearChoice = { 
/**
 * The category's id.
 */
id: string, 
/**
 * The user confirmed clearing this category. Required for `data` categories, ignored for `cache`.
 */
confirmed: boolean, };

export type ClearRequest = { 
/**
 * The categories (at most [`crate::MAX_CATEGORIES`]).
 */
categories: Array<ClearChoice>, };

export type StorageClearResult = { 
/**
 * Bytes freed in all categories.
 */
freedBytes: number, 
/**
 * One entry per requested category.
 */
categories: Array<CategoryClearResult>, 
/**
 * Stopped by Cancel before everything was cleared (what was done stays done).
 */
cancelled: boolean, };

export type SizeNode = { 
/**
 * The file or folder name; empty for a folded node (the UI words it: "N other items").
 */
name: string, 
/**
 * Bytes in it and everything below it.
 */
bytes: number, 
/**
 * How many items this node stands for when it is a fold; 0 for a real item.
 */
folded: number, 
/**
 * The biggest items below it, biggest first (none below the tree's depth).
 */
children: Array<SizeNode>, };

export type StorageTree = { 
/**
 * The category.
 */
id: string, 
/**
 * The category's folder as a node named after the category.
 */
root: SizeNode, 
/**
 * The walk stopped at its entry or time limit, or was cancelled: the sizes are lower bounds.
 */
capped: boolean, };

export type StorageEvent = { "state": "usage", 
/**
 * What the app keeps.
 */
usage: StorageUsage, } | { "state": "cleared", 
/**
 * What it did.
 */
result: StorageClearResult, } | { "state": "tree", 
/**
 * The category's tree.
 */
tree: StorageTree, } | { "state": "cancelled" } | { "state": "failed", 
/**
 * Words for the user.
 */
message: string, };

/** The event the outcome of a scan or a clear is sent on, to every window (a `StorageEvent`). */
export const STORAGE_EVENT = 'paddock://storage';
