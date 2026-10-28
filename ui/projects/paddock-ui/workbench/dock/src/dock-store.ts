import { InjectionToken } from '@angular/core';

/**
 * Where the dock keeps its documents (the current layout, the named layouts, the perspectives' arrangements): a small
 * key/value port. A key matches `^[a-z0-9._:-]{1,64}$` and a value is a JSON object of at most 1 MiB (the engine's
 * `layout_set` refuses anything else). Implementations never throw from `save`; `load` resolves `null` when there is
 * nothing (or nothing readable), so a corrupt store means the default layout, never a crash.
 */
export interface DockLayoutStore {
  load(key: string): Promise<unknown | null>;
  save(key: string, value: Record<string, unknown>): Promise<void>;
}

/** What the engine accepts as a key. */
export const LAYOUT_KEY_PATTERN = /^[a-z0-9._:-]{1,64}$/;
/** The largest value the engine stores, in bytes of its JSON text. */
export const LAYOUT_VALUE_MAX = 1_000_000;

/** Whether `key` and `value` would be accepted by the engine (checked before any write, so a refusal is local and quiet). */
export function layoutEntryProblem(key: string, value: unknown): string | null {
  if (!LAYOUT_KEY_PATTERN.test(key)) return `layout key "${key}" must match ${LAYOUT_KEY_PATTERN.source}`;
  if (typeof value !== 'object' || value === null || Array.isArray(value)) return 'a layout value must be a JSON object';
  let text: string;
  try {
    text = JSON.stringify(value);
  } catch {
    return 'a layout value must be JSON';
  }
  return text.length > LAYOUT_VALUE_MAX ? `a layout value is limited to ${LAYOUT_VALUE_MAX} bytes` : null;
}

/** Keeps the documents in memory: tests and windows that do not persist. */
export class InMemoryDockLayoutStore implements DockLayoutStore {
  readonly entries = new Map<string, string>();

  load(key: string): Promise<unknown | null> {
    const text = this.entries.get(key);
    if (text === undefined) return Promise.resolve(null);
    try {
      return Promise.resolve(JSON.parse(text));
    } catch {
      return Promise.resolve(null);
    }
  }

  save(key: string, value: Record<string, unknown>): Promise<void> {
    if (layoutEntryProblem(key, value) === null) this.entries.set(key, JSON.stringify(value));
    return Promise.resolve();
  }
}

/** Keeps the documents in the browser's `localStorage` (the demo); every access is guarded. */
export class BrowserDockLayoutStore implements DockLayoutStore {
  constructor(private readonly prefix = 'paddock.dock.') {}

  load(key: string): Promise<unknown | null> {
    try {
      const raw = localStorage.getItem(this.prefix + key);
      return Promise.resolve(raw ? (JSON.parse(raw) as unknown) : null);
    } catch {
      return Promise.resolve(null);
    }
  }

  save(key: string, value: Record<string, unknown>): Promise<void> {
    try {
      if (layoutEntryProblem(key, value) === null) localStorage.setItem(this.prefix + key, JSON.stringify(value));
    } catch {
      /* storage unavailable or full: the layout is a convenience */
    }
    return Promise.resolve();
  }
}

/** Tauri's `invoke` (or any function of its shape). */
export type DockInvoke = <T>(command: string, args?: Record<string, unknown>) => Promise<T>;

/** The `paddock-workbench` plugin's layout commands. */
export const LAYOUT_GET = 'plugin:paddock-workbench|layout_get';
export const LAYOUT_SET = 'plugin:paddock-workbench|layout_set';

/** Keeps the documents in the engine (`paddock-workbench`): `layout_get { key }` gives the value or null, `layout_set { key, value }` stores it. */
export class IpcDockLayoutStore implements DockLayoutStore {
  constructor(private readonly invoke: DockInvoke) {}

  async load(key: string): Promise<unknown | null> {
    if (!LAYOUT_KEY_PATTERN.test(key)) return null;
    try {
      return (await this.invoke<unknown | null>(LAYOUT_GET, { key })) ?? null;
    } catch {
      return null;
    }
  }

  async save(key: string, value: Record<string, unknown>): Promise<void> {
    if (layoutEntryProblem(key, value) !== null) return;
    try {
      await this.invoke<void>(LAYOUT_SET, { key, value });
    } catch {
      /* the engine refused or is away: the layout is a convenience, the next change tries again */
    }
  }
}

/** The window's store; the default keeps documents in memory. `provideWorkbenchDock({ store })` replaces it. */
export const DOCK_LAYOUT_STORE = new InjectionToken<DockLayoutStore>('PADDOCK_DOCK_LAYOUT_STORE');
