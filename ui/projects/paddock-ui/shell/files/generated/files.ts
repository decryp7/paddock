// Generated from the Rust wire types of paddock-files (ts-rs). Do not edit: PADDOCK_WRITE_TS=1 cargo test -p paddock-files --all-features --lib generated.

export type PickKind = "openFile" | "openFiles" | "openFolder" | "openFolders" | "save";

export type PickFilter = { 
/**
 * What the user sees ("Log files").
 */
name: string, 
/**
 * Extensions without the dot (`log`, `txt`); letters and digits only.
 */
extensions: Array<string>, };

export type PickRequest = { 
/**
 * What to choose.
 */
kind: PickKind, 
/**
 * The dialog's title (empty: the OS's).
 */
title: string, 
/**
 * The file types offered.
 */
filters?: Array<PickFilter>, 
/**
 * The suggested file name of a save dialog (a name, not a path).
 */
defaultName?: string, };

export type AppFolder = "data" | "config" | "cache" | "logs";

export type RecentEntry = { 
/**
 * The absolute path as it was granted.
 */
path: string, 
/**
 * A display name (a project's), when it has one.
 */
name?: string, 
/**
 * When it was last opened or saved, in milliseconds since the Unix epoch.
 */
openedAt: number, };

export type RecentList = { 
/**
 * The list's id.
 */
id: string, 
/**
 * The entries, newest first.
 */
entries: Array<RecentEntry>, };

export type RecentLists = { 
/**
 * One entry per declared list.
 */
lists: Array<RecentList>, };

export type DroppedFiles = { 
/**
 * The paths that were granted to the window (files and folders).
 */
paths: Array<string>, };

/** Sent to every window when a recent list changed (a `RecentLists`). */
export const FILES_RECENT_EVENT = 'paddock://files-recent';

/** Sent to a window when files dropped on it were granted (a `DroppedFiles`). */
export const FILES_DROPPED_EVENT = 'paddock://files-dropped';
