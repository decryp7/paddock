import type { AppFolder, DroppedFiles, RecentEntry, RecentList, RecentLists } from '../generated/files';

// Payloads from the engine are parsed once, at this edge: what is not the expected shape is dropped, and what passes is rebuilt
// from known fields only (an unknown extra field never travels on).

function isObject(v: unknown): v is Record<string, unknown> {
  return typeof v === 'object' && v !== null && !Array.isArray(v);
}

function parseEntry(v: unknown): RecentEntry | null {
  if (!isObject(v) || typeof v['path'] !== 'string' || v['path'] === '' || typeof v['openedAt'] !== 'number' || !Number.isFinite(v['openedAt'])) return null;
  const entry: RecentEntry = { path: v['path'], openedAt: v['openedAt'] };
  if (typeof v['name'] === 'string' && v['name'] !== '') entry.name = v['name'];
  return entry;
}

function parseList(v: unknown): RecentList | null {
  if (!isObject(v) || typeof v['id'] !== 'string' || !Array.isArray(v['entries'])) return null;
  const entries = v['entries'].map(parseEntry).filter((e): e is RecentEntry => e !== null);
  return { id: v['id'], entries };
}

/** The recent lists from `files_recent_list` or the change event, or null when the payload is not one. */
export function parseRecentLists(v: unknown): RecentLists | null {
  if (!isObject(v) || !Array.isArray(v['lists'])) return null;
  const lists = v['lists'].map(parseList);
  if (lists.some((l) => l === null)) return null;
  return { lists: lists as RecentList[] };
}

/** The paths granted to this window by a drop, or null when the payload is not one. */
export function parseDroppedFiles(v: unknown): DroppedFiles | null {
  if (!isObject(v) || !Array.isArray(v['paths']) || !v['paths'].every((p) => typeof p === 'string')) return null;
  return { paths: v['paths'] as string[] };
}

/** The entries of list `id` (empty when the list is unknown). */
export function entriesOf(lists: RecentLists | null, id: string): readonly RecentEntry[] {
  return lists?.lists.find((l) => l.id === id)?.entries ?? [];
}

/** The app folders, for validation at the boundary of a page that lets the user pick one. */
export const APP_FOLDERS: readonly AppFolder[] = ['data', 'config', 'cache', 'logs'];
