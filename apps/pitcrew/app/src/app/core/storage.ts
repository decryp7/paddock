/** Per-viewer conveniences in `localStorage`. Storage can be missing or throw (private mode, blocked site data). */

export function readLocal(key: string): string | null {
  try {
    return globalThis.localStorage?.getItem(key) ?? null;
  } catch {
    return null;
  }
}

export function writeLocal(key: string, value: string): void {
  try {
    globalThis.localStorage?.setItem(key, value);
  } catch {
    // Not persisted; the setting still applies for this session.
  }
}

/** Parses stored JSON, falling back on anything missing or damaged. */
export function readLocalJson<T>(key: string, fallback: T, guard: (v: unknown) => v is T): T {
  const raw = readLocal(key);
  if (raw === null) return fallback;
  try {
    const v: unknown = JSON.parse(raw);
    return guard(v) ? v : fallback;
  } catch {
    return fallback;
  }
}
