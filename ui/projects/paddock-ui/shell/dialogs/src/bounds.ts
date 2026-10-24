import type { DialogRemembered } from '@decryp7/paddock-ui/shell';

/** The storage key of the remembered places (no app name: each app has its own origin). */
export const DIALOG_BOUNDS_KEY = 'paddock.dialog.bounds';
/** The stored shape's version: an offset from the parent window plus a size, never an absolute screen position. */
export const DIALOG_BOUNDS_VERSION = 1;
/** More remembered dialogs than this are never stored (a corrupt value cannot grow without bound). */
export const MAX_REMEMBERED = 200;

/** Storage as the bounds need it (`localStorage`, or a fake). */
export type BoundsStorage = Pick<Storage, 'getItem' | 'setItem'>;

export function isRememberedPlace(v: unknown): v is DialogRemembered {
  if (typeof v !== 'object' || v === null) return false;
  const r = v as Record<string, unknown>;
  return (
    ['dx', 'dy', 'width', 'height'].every((k) => typeof r[k] === 'number' && Number.isFinite(r[k])) &&
    (r['width'] as number) > 0 &&
    (r['height'] as number) > 0
  );
}

/**
 * What is remembered of each dialog (logical px): its size and its offset from its parent window, by dialog id. Rust
 * decides where the dialog goes (on the parent's monitor) and uses the offset only when the whole dialog stays there.
 * Missing, corrupt, old or foreign data falls back to nothing.
 */
export function loadDialogBounds(storage: Pick<Storage, 'getItem'> | null = safeStorage()): Record<string, DialogRemembered> {
  try {
    const raw = JSON.parse(storage?.getItem(DIALOG_BOUNDS_KEY) ?? 'null') as { version?: unknown; bounds?: unknown } | null;
    if (!raw || raw.version !== DIALOG_BOUNDS_VERSION || typeof raw.bounds !== 'object' || raw.bounds === null) return {};
    const out: Record<string, DialogRemembered> = {};
    for (const [id, r] of Object.entries(raw.bounds as Record<string, unknown>).slice(0, MAX_REMEMBERED)) {
      if (isRememberedPlace(r)) out[id] = { dx: r.dx, dy: r.dy, width: r.width, height: r.height };
    }
    return out;
  } catch {
    return {};
  }
}

/** Remembers `place` for dialog `id`. Never throws (a convenience only). */
export function rememberDialogBounds(id: string, place: DialogRemembered, storage: BoundsStorage | null = safeStorage()): void {
  if (!isRememberedPlace(place)) return;
  try {
    const all = loadDialogBounds(storage);
    delete all[id];
    const kept = Object.entries(all).slice(-(MAX_REMEMBERED - 1));
    const bounds = Object.fromEntries([...kept, [id, { dx: place.dx, dy: place.dy, width: place.width, height: place.height }]]);
    storage?.setItem(DIALOG_BOUNDS_KEY, JSON.stringify({ version: DIALOG_BOUNDS_VERSION, bounds }));
  } catch {
    /* a convenience only */
  }
}

function safeStorage(): Storage | null {
  try {
    return typeof localStorage === 'undefined' ? null : localStorage;
  } catch {
    return null;
  }
}
