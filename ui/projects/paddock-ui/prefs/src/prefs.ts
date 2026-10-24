/**
 * Per-viewer conveniences in `localStorage` (a remembered tab or filter, a collapsed section, an unsent draft), plan X35.
 * Storage can be missing or throw (private mode, blocked site data, a quota), so every read and write is guarded: a read
 * that fails is "nothing stored", a write that fails is silently not persisted and the value still applies for this
 * session. Never use it for state that must persist reliably or be shared: that belongs to the engine.
 */

/** The part of `Storage` that preferences use; tests pass `memoryStorage()`. */
export interface PrefsStorage {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
  removeItem(key: string): void;
}

/** A storage in memory: tests, and the fallback when the browser's storage is missing. */
export function memoryStorage(initial: Readonly<Record<string, string>> = {}): PrefsStorage {
  const map = new Map(Object.entries(initial));
  return {
    getItem: (key) => map.get(key) ?? null,
    setItem: (key, value) => void map.set(key, value),
    removeItem: (key) => void map.delete(key),
  };
}

/** The browser's `localStorage`, or null where it is missing or its accessor throws. */
function browserStorage(): PrefsStorage | null {
  try {
    return globalThis.localStorage ?? null;
  } catch {
    return null;
  }
}

/** Guarded reads and writes over one storage. */
export interface Prefs {
  /** The stored text, or null when nothing is stored or the storage is not available. */
  read(key: string): string | null;
  /** Stores text; a failing storage is ignored. Returns whether it was stored. */
  write(key: string, value: string): boolean;
  /** Removes a key; a failing storage is ignored. */
  remove(key: string): void;
  /** Parses stored JSON; anything missing, damaged or rejected by `guard` gives `fallback`. */
  readJson<T>(key: string, fallback: T, guard: (value: unknown) => value is T): T;
  /** Stores a value as JSON. Returns whether it was stored. */
  writeJson(key: string, value: unknown): boolean;
}

/** Guarded preferences over `storage` (default: the browser's `localStorage`, looked up on every call). */
export function createPrefs(storage?: PrefsStorage): Prefs {
  const store = (): PrefsStorage | null => storage ?? browserStorage();
  const prefs: Prefs = {
    read(key) {
      try {
        return store()?.getItem(key) ?? null;
      } catch {
        return null;
      }
    },
    write(key, value) {
      try {
        const s = store();
        if (!s) return false;
        s.setItem(key, value);
        return true;
      } catch {
        return false;
      }
    },
    remove(key) {
      try {
        store()?.removeItem(key);
      } catch {
        // Nothing to remove from a storage that is not there.
      }
    },
    readJson<T>(key: string, fallback: T, guard: (value: unknown) => value is T): T {
      const raw = prefs.read(key);
      if (raw === null) return fallback;
      try {
        const value: unknown = JSON.parse(raw);
        return guard(value) ? value : fallback;
      } catch {
        return fallback;
      }
    },
    writeJson(key, value) {
      try {
        return prefs.write(key, JSON.stringify(value));
      } catch {
        return false;
      }
    },
  };
  return prefs;
}

/** The default guarded preferences over `localStorage`. */
export const localPrefs: Prefs = createPrefs();
