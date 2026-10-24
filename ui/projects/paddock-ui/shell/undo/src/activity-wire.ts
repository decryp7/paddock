/** The Tauri event the app emits an activity entry on when it is recorded or changes (`paddock://<area>`, ADR 0012). */
export const ACTIVITY_EVENT = 'paddock://activity';

export type ActivityUndoState = 'notUndoable' | 'available' | 'undone' | 'expired';

/** One logged action: the wire model of `paddock_jobs::journal::Entry` (camelCase). */
export interface ActivityEntry {
  readonly id: number;
  /** Milliseconds since the Unix epoch. */
  readonly atMs: number;
  /** The app's tag for the kind of action (`rule`, `cleanup`, ...). */
  readonly kind: string;
  readonly title: string;
  readonly detail: string;
  readonly undo: ActivityUndoState;
  readonly bytes: number | null;
}

const STATES: readonly string[] = ['notUndoable', 'available', 'undone', 'expired'];

/** Reads one entry at the edge; anything that is not an entry is dropped (`null`), never half-trusted. */
export function parseActivityEntry(value: unknown): ActivityEntry | null {
  if (typeof value !== 'object' || value === null) return null;
  const v = value as Record<string, unknown>;
  const { id, atMs, kind, title, detail, undo, bytes } = v;
  if (typeof id !== 'number' || !Number.isSafeInteger(id) || id < 0) return null;
  if (typeof atMs !== 'number' || !Number.isFinite(atMs)) return null;
  if (typeof kind !== 'string' || typeof title !== 'string' || typeof detail !== 'string') return null;
  if (typeof undo !== 'string' || !STATES.includes(undo)) return null;
  if (bytes !== null && bytes !== undefined && (typeof bytes !== 'number' || !Number.isFinite(bytes) || bytes < 0)) return null;
  return { id, atMs, kind, title, detail, undo: undo as ActivityUndoState, bytes: typeof bytes === 'number' ? bytes : null };
}

/** Reads a list of entries; the ones that are not entries are dropped. A payload that is not a list is empty. */
export function parseActivityEntries(value: unknown): ActivityEntry[] {
  return Array.isArray(value) ? value.map(parseActivityEntry).filter((e): e is ActivityEntry => e !== null) : [];
}
