import { toCommandError } from '@decryp7/paddock-ui/ipc';
import type { CommandError } from '@decryp7/paddock-ui/ipc';
import { FILES_DROPPED_EVENT, FILES_RECENT_EVENT, type AppFolder, type DroppedFiles, type PickRequest, type RecentLists } from '../generated/files';
import type { FilesCommand } from '../generated/files-commands';
import { entriesOf, parseDroppedFiles, parseRecentLists } from './wire';

/** Stops listening. */
export type Unlisten = () => void;

/**
 * The app's IPC as this backend needs it, so this package does not depend on `@tauri-apps/api`: pass Tauri's `invoke` and an
 * adapter over its `listen` (`(event, cb) => listen(event, (e) => cb(e.payload))`).
 */
export interface FilesIpc {
  invoke<T>(command: FilesCommand, args?: Record<string, unknown>): Promise<T>;
  listen(event: string, handler: (payload: unknown) => void): Promise<Unlisten>;
}

/**
 * The user's files (`paddock_files::tauri`). The engine decides which paths the app may use: the UI gets a path only from
 * `pick`, a drop or the recent lists, and every other call checks it again in Rust. Every call may reject with a `CommandError`
 * (`notGranted` for a path nobody chose).
 */
export abstract class FilesBackend {
  /** Shows the native dialog for this window; the chosen paths (empty when it was closed). */
  abstract pick(request: PickRequest): Promise<string[]>;
  /** The recent lists. */
  abstract recent(): Promise<RecentLists>;
  /** Takes a path out of a list. */
  abstract removeRecent(list: string, path: string): Promise<void>;
  /** Empties a list. */
  abstract clearRecent(list: string): Promise<void>;
  /** Grants a path of a list to this window again; the canonical path. */
  abstract regrant(list: string, path: string): Promise<string>;
  /** Shows a granted path in the file manager. */
  abstract reveal(path: string): Promise<void>;
  /** Opens a granted path with the OS's program for it. */
  abstract open(path: string): Promise<void>;
  /** Opens a granted file in the editor at a line (1 to ten million); the file is an argument of the editor, never run. */
  abstract openAtLine(path: string, line: number): Promise<void>;
  /** Shows one of the app's own folders in the file manager. */
  abstract openAppFolder(folder: AppFolder): Promise<void>;
  /** Opens a web address in the browser. */
  abstract openUrl(url: string): Promise<void>;
  /** Every change of the recent lists until the returned function is called. */
  abstract listenRecent(handler: (lists: RecentLists) => void): Promise<Unlisten>;
  /** Every drop on this window (already granted) until the returned function is called. */
  abstract listenDropped(handler: (dropped: DroppedFiles) => void): Promise<Unlisten>;
}

/** User files over Tauri. Payloads are parsed at this edge. */
export class TauriFilesBackend extends FilesBackend {
  constructor(private readonly ipc: FilesIpc) {
    super();
  }

  async pick(request: PickRequest): Promise<string[]> {
    const paths = await this.ipc.invoke<unknown>('files_pick', { request });
    return Array.isArray(paths) ? paths.filter((p): p is string => typeof p === 'string') : [];
  }

  async recent(): Promise<RecentLists> {
    const lists = parseRecentLists(await this.ipc.invoke<unknown>('files_recent_list'));
    if (!lists) throw new Error('The engine sent no valid recent lists.');
    return lists;
  }

  async removeRecent(list: string, path: string): Promise<void> {
    await this.ipc.invoke<unknown>('files_recent_remove', { list, path });
  }

  async clearRecent(list: string): Promise<void> {
    await this.ipc.invoke<unknown>('files_recent_clear', { list });
  }

  async regrant(list: string, path: string): Promise<string> {
    const granted = await this.ipc.invoke<unknown>('files_recent_grant', { list, path });
    if (typeof granted !== 'string') throw new Error('The engine sent no path.');
    return granted;
  }

  async reveal(path: string): Promise<void> {
    await this.ipc.invoke<unknown>('files_reveal', { path });
  }

  async open(path: string): Promise<void> {
    await this.ipc.invoke<unknown>('files_open', { path });
  }

  async openAtLine(path: string, line: number): Promise<void> {
    await this.ipc.invoke<unknown>('files_open_at_line', { path, line });
  }

  async openAppFolder(folder: AppFolder): Promise<void> {
    await this.ipc.invoke<unknown>('files_open_app_folder', { folder });
  }

  async openUrl(url: string): Promise<void> {
    await this.ipc.invoke<unknown>('files_open_url', { url });
  }

  listenRecent(handler: (lists: RecentLists) => void): Promise<Unlisten> {
    return this.ipc.listen(FILES_RECENT_EVENT, (payload) => {
      const lists = parseRecentLists(payload);
      if (lists) handler(lists);
    });
  }

  listenDropped(handler: (dropped: DroppedFiles) => void): Promise<Unlisten> {
    return this.ipc.listen(FILES_DROPPED_EVENT, (payload) => {
      const dropped = parseDroppedFiles(payload);
      if (dropped) handler(dropped);
    });
  }
}

/** What the in-memory engine does. */
export interface MemoryFilesOptions {
  /** The lists at the start (every list the app declares; default: `files` and `projects`, empty). */
  readonly lists?: RecentLists;
  /** What the dialog answers, one answer per `pick` (an empty answer: the user closed it). */
  readonly choices?: string[][];
  /** Paths already granted to the window. */
  readonly granted?: readonly string[];
  /** Commands that reject, with the error they reject with. */
  readonly fail?: Partial<Record<FilesCommand, CommandError>>;
}

/** The refusal of a path nobody chose, as the engine words it. */
export function notGranted(path: string): CommandError {
  const name = path.split(/[\\/]/).pop() ?? path;
  return { code: 'notGranted', message: `Only files you choose can be read: choose ${name} again with Open`, retryable: false, hint: 'Choose it again in the dialog' };
}

/**
 * The files engine in memory, for the browser build and tests: it keeps the rules of the Rust side (a path is used only after
 * a dialog, a drop or the recent list granted it) and records what it was asked to do, so a test can assert on it. It never
 * touches a real file.
 */
export class MemoryFilesBackend extends FilesBackend {
  /** Every call, in order, as `[command, args]`. */
  readonly calls: [FilesCommand, Record<string, unknown> | undefined][] = [];
  /** The paths shown in the file manager. */
  readonly revealed: string[] = [];
  /** The paths opened with their program. */
  readonly launched: string[] = [];
  /** The files opened in the editor, as `path:line`. */
  readonly edited: string[] = [];
  /** The app folders shown. */
  readonly folders: AppFolder[] = [];
  /** The web addresses opened. */
  readonly urls: string[] = [];
  private lists: RecentLists;
  private readonly granted: Set<string>;
  private readonly choices: string[][];
  private readonly fail: Partial<Record<FilesCommand, CommandError>>;
  private readonly recentHandlers = new Set<(lists: RecentLists) => void>();
  private readonly droppedHandlers = new Set<(dropped: DroppedFiles) => void>();

  constructor(options: MemoryFilesOptions = {}) {
    super();
    this.lists = options.lists ?? { lists: [{ id: 'files', entries: [] }, { id: 'projects', entries: [] }] };
    this.granted = new Set(options.granted ?? []);
    this.choices = [...(options.choices ?? [])];
    this.fail = options.fail ?? {};
  }

  /** Whether an answer is queued for the next dialog. */
  hasChoice(): boolean {
    return this.choices.length > 0;
  }

  /** Queues the answer of the next dialog. */
  choose(...paths: string[]): void {
    this.choices.push(paths);
  }

  /** The app opened `path` (it was granted): records it in `list` and announces the change, like `paddock_files::tauri::opened`. */
  recordOpened(list: string, path: string, name?: string, now = Date.now()): void {
    this.granted.add(path);
    this.lists = {
      lists: this.lists.lists.map((l) =>
        l.id === list ? { ...l, entries: [{ path, openedAt: now, ...(name ? { name } : {}) }, ...l.entries.filter((e) => e.path !== path)] } : l,
      ),
    };
    this.announce();
  }

  /** The user dropped files on the window: they are granted and announced. */
  drop(...paths: string[]): void {
    for (const p of paths) this.granted.add(p);
    for (const h of [...this.droppedHandlers]) h({ paths });
  }

  /** Whether the window may use `path` now. */
  isGranted(path: string): boolean {
    return this.granted.has(path);
  }

  private call(command: FilesCommand, args?: Record<string, unknown>): void {
    this.calls.push([command, args]);
    const error = this.fail[command];
    if (error) throw error;
  }

  private announce(): void {
    for (const h of [...this.recentHandlers]) h(this.lists);
  }

  private require(path: string): void {
    if (!this.granted.has(path)) throw notGranted(path);
  }

  pick(request: PickRequest): Promise<string[]> {
    return this.run(() => {
      this.call('files_pick', { request });
      const chosen = this.choices.shift() ?? [];
      for (const p of chosen) this.granted.add(p);
      return chosen;
    });
  }

  recent(): Promise<RecentLists> {
    return this.run(() => {
      this.call('files_recent_list');
      return this.lists;
    });
  }

  removeRecent(list: string, path: string): Promise<void> {
    return this.run(() => {
      this.call('files_recent_remove', { list, path });
      const before = this.lists;
      this.lists = { lists: before.lists.map((l) => (l.id === list ? { ...l, entries: l.entries.filter((e) => e.path !== path) } : l)) };
      if (entriesOf(before, list).length !== entriesOf(this.lists, list).length) this.announce();
    });
  }

  clearRecent(list: string): Promise<void> {
    return this.run(() => {
      this.call('files_recent_clear', { list });
      const had = entriesOf(this.lists, list).length > 0;
      this.lists = { lists: this.lists.lists.map((l) => (l.id === list ? { ...l, entries: [] } : l)) };
      if (had) this.announce();
    });
  }

  regrant(list: string, path: string): Promise<string> {
    return this.run(() => {
      this.call('files_recent_grant', { list, path });
      if (!entriesOf(this.lists, list).some((e) => e.path === path)) {
        throw { code: 'notFound', message: 'This file is no longer in the recent list; choose it again with Open', retryable: false } satisfies CommandError;
      }
      this.granted.add(path);
      return path;
    });
  }

  reveal(path: string): Promise<void> {
    return this.run(() => {
      this.call('files_reveal', { path });
      this.require(path);
      this.revealed.push(path);
    });
  }

  open(path: string): Promise<void> {
    return this.run(() => {
      this.call('files_open', { path });
      this.require(path);
      this.launched.push(path);
    });
  }

  openAtLine(path: string, line: number): Promise<void> {
    return this.run(() => {
      this.call('files_open_at_line', { path, line });
      if (!Number.isInteger(line) || line < 1 || line > 10_000_000) {
        throw { code: 'invalid', message: 'the line must be between 1 and 10000000', retryable: false, field: 'line' } satisfies CommandError;
      }
      this.require(path);
      this.edited.push(`${path}:${line}`);
    });
  }

  openAppFolder(folder: AppFolder): Promise<void> {
    return this.run(() => {
      this.call('files_open_app_folder', { folder });
      this.folders.push(folder);
    });
  }

  openUrl(url: string): Promise<void> {
    return this.run(() => {
      this.call('files_open_url', { url });
      if (!/^(https?:\/\/[^\s/?#]+|mailto:\S+)/i.test(url) || /\s/.test(url)) {
        throw { code: 'invalid', message: 'this web address cannot be opened (only http, https and mailto addresses are opened)', retryable: false, field: 'url' } satisfies CommandError;
      }
      this.urls.push(url);
    });
  }

  listenRecent(handler: (lists: RecentLists) => void): Promise<Unlisten> {
    this.recentHandlers.add(handler);
    return Promise.resolve(() => void this.recentHandlers.delete(handler));
  }

  listenDropped(handler: (dropped: DroppedFiles) => void): Promise<Unlisten> {
    this.droppedHandlers.add(handler);
    return Promise.resolve(() => void this.droppedHandlers.delete(handler));
  }

  /** Runs a synchronous engine step as a promise, rejecting with the typed error it throws. */
  private run<T>(step: () => T): Promise<T> {
    try {
      return Promise.resolve(step());
    } catch (e) {
      return Promise.reject(toCommandError(e));
    }
  }
}
