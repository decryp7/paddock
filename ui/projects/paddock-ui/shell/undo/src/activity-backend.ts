import { ACTIVITY_EVENT, parseActivityEntries, parseActivityEntry, type ActivityEntry } from './activity-wire';

/** Stops listening. */
export type Unlisten = () => void;

/** The newest entries the page asks for. */
export const ACTIVITY_PAGE_SIZE = 200;

/**
 * The activity port: the app's journal commands in the desktop app, a fake in the browser build and in tests. The journal
 * (`paddock_jobs::journal`) records; *undoing* an entry is the app's own work (it knows how), so the app owns both commands
 * and calls `Journal::mark` when the undo is done. Components and stores never call `invoke`.
 */
export abstract class ActivityBackend {
  /** The newest `limit` entries, newest first. */
  abstract list(limit: number): Promise<readonly ActivityEntry[]>;
  /** Undoes entry `id`; resolves with the entry as it is now (`undone`) or rejects with a message for the user. */
  abstract undo(id: number): Promise<ActivityEntry>;
  /** Calls `handler` with every entry that is recorded or changes, until the returned function is called. */
  abstract listen(handler: (entry: ActivityEntry) => void): Promise<Unlisten>;
}

/** The command names the app registered for its journal (defaults `activity_list` and `activity_undo`). */
export interface ActivityCommands {
  readonly list: string;
  readonly undo: string;
}

export const DEFAULT_ACTIVITY_COMMANDS: ActivityCommands = { list: 'activity_list', undo: 'activity_undo' };

/** The app's IPC as the backend needs it, so this package does not depend on `@tauri-apps/api`. */
export interface ActivityIpc {
  invoke<T>(command: string, args?: Record<string, unknown>): Promise<T>;
  listen(event: string, handler: (payload: unknown) => void): Promise<Unlisten>;
}

/** The activity backend over Tauri. Payloads are parsed at this edge. */
export class TauriActivityBackend extends ActivityBackend {
  constructor(
    private readonly ipc: ActivityIpc,
    private readonly commands: ActivityCommands = DEFAULT_ACTIVITY_COMMANDS,
  ) {
    super();
  }

  async list(limit: number): Promise<readonly ActivityEntry[]> {
    return parseActivityEntries(await this.ipc.invoke<unknown>(this.commands.list, { limit }));
  }

  async undo(id: number): Promise<ActivityEntry> {
    const entry = parseActivityEntry(await this.ipc.invoke<unknown>(this.commands.undo, { id }));
    if (!entry) throw new Error($localize`:@@pd.activity.badAnswer:The app did not answer with the entry.`);
    return entry;
  }

  listen(handler: (entry: ActivityEntry) => void): Promise<Unlisten> {
    return this.ipc.listen(ACTIVITY_EVENT, (payload) => {
      const entry = parseActivityEntry(payload);
      if (entry) handler(entry);
    });
  }
}

/**
 * A backend held in memory (browser build, demos, tests): `record` plays the journal, `onUndo` plays the app's undo (it
 * may throw to refuse; by default the entry just becomes `undone`).
 */
export class MemoryActivityBackend extends ActivityBackend {
  private readonly entries = new Map<number, ActivityEntry>();
  private readonly listeners = new Set<(entry: ActivityEntry) => void>();
  private next = 1;

  constructor(private readonly onUndo: (entry: ActivityEntry) => void = () => undefined) {
    super();
  }

  /** Records an entry as the journal would and tells the listeners. */
  record(entry: Partial<Omit<ActivityEntry, 'id'>> & Pick<ActivityEntry, 'title'>): ActivityEntry {
    const full: ActivityEntry = { id: this.next++, atMs: 0, kind: 'system', detail: '', undo: 'notUndoable', bytes: null, ...entry };
    this.put(full);
    return full;
  }

  private put(entry: ActivityEntry): void {
    this.entries.set(entry.id, entry);
    for (const l of [...this.listeners]) l(entry);
  }

  list(limit: number): Promise<readonly ActivityEntry[]> {
    return Promise.resolve([...this.entries.values()].sort((a, b) => b.id - a.id).slice(0, limit));
  }

  undo(id: number): Promise<ActivityEntry> {
    const entry = this.entries.get(id);
    if (!entry) return Promise.reject(new Error($localize`:@@pd.activity.gone:That entry is no longer in the log.`));
    if (entry.undo !== 'available') return Promise.reject(new Error($localize`:@@pd.activity.cannot:That action cannot be undone any more.`));
    try {
      this.onUndo(entry);
    } catch (e) {
      return Promise.reject(e instanceof Error ? e : new Error(String(e)));
    }
    const undone: ActivityEntry = { ...entry, undo: 'undone' };
    this.put(undone);
    return Promise.resolve(undone);
  }

  listen(handler: (entry: ActivityEntry) => void): Promise<Unlisten> {
    this.listeners.add(handler);
    return Promise.resolve(() => void this.listeners.delete(handler));
  }
}
