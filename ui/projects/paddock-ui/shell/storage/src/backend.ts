import { toCommandError, type CommandError } from '@decryp7/paddock-ui/ipc';
import { JOBS_EVENT, parseJobSnapshot } from '@decryp7/paddock-ui/shell/jobs';
import { STORAGE_EVENT, type CategoryClearResult, type ClearRequest, type StorageCategory, type StorageClearResult, type StorageEvent, type StorageUsage } from '../generated/storage';
import type { StorageCommand } from '../generated/storage-commands';
import { parseStorageEvent, parseStorageUsage } from './wire';

/** Stops listening. */
export type Unlisten = () => void;

/** Where a running scan or clear is: the phase's label and its count (`total` is null while it is not known). */
export interface StorageProgress {
  readonly label: string;
  readonly done: number;
  readonly total: number | null;
}

/**
 * The app's IPC as this backend needs it, so this package does not depend on `@tauri-apps/api`: pass Tauri's `invoke` and an
 * adapter over its `listen` (`(event, cb) => listen(event, (e) => cb(e.payload))`).
 */
export interface StorageIpc {
  invoke<T>(command: StorageCommand, args?: Record<string, unknown>): Promise<T>;
  listen(event: string, handler: (payload: unknown) => void): Promise<Unlisten>;
}

/**
 * What the app keeps on disk (`paddock_storage::tauri`). The engine decides and acts; this only asks and shows. A scan and a
 * clear start a job and return at once: the outcome arrives by `listen`, the live phase by `listenProgress`. Every command may
 * reject with a `CommandError` (`busy` while another job runs, `invalid` with kind `noPreview` before the first scan).
 */
export abstract class StorageBackend {
  /** The last measured usage, or null when the app has not measured since it started. */
  abstract last(): Promise<StorageUsage | null>;
  /** Starts measuring; the outcome follows as an event. */
  abstract scan(): Promise<void>;
  /** Starts clearing; the outcome follows as an event. */
  abstract clear(request: ClearRequest): Promise<void>;
  /** Cancels the job in progress; whether there was one. */
  abstract cancel(): Promise<boolean>;
  /** Shows a category's folder in the file manager. */
  abstract reveal(id: string): Promise<void>;
  /** Every outcome the engine sends until the returned function is called. */
  abstract listen(handler: (event: StorageEvent) => void): Promise<Unlisten>;
  /** The phase of the scan or clear in progress (null when it ended) until the returned function is called. */
  abstract listenProgress(handler: (progress: StorageProgress | null) => void): Promise<Unlisten>;
}

/** Storage over Tauri. Payloads are parsed at this edge. */
export class TauriStorageBackend extends StorageBackend {
  constructor(private readonly ipc: StorageIpc) {
    super();
  }

  async last(): Promise<StorageUsage | null> {
    const v = await this.ipc.invoke<unknown>('storage_last');
    if (v === null || v === undefined) return null;
    const usage = parseStorageUsage(v);
    if (!usage) throw new Error('The engine sent no valid storage figures.');
    return usage;
  }

  async scan(): Promise<void> {
    await this.ipc.invoke<unknown>('storage_scan');
  }

  async clear(request: ClearRequest): Promise<void> {
    await this.ipc.invoke<unknown>('storage_clear', { request });
  }

  async cancel(): Promise<boolean> {
    return (await this.ipc.invoke<unknown>('storage_cancel')) === true;
  }

  async reveal(id: string): Promise<void> {
    await this.ipc.invoke<unknown>('storage_reveal', { id });
  }

  listen(handler: (event: StorageEvent) => void): Promise<Unlisten> {
    return this.ipc.listen(STORAGE_EVENT, (payload) => {
      const event = parseStorageEvent(payload);
      if (event) handler(event);
    });
  }

  listenProgress(handler: (progress: StorageProgress | null) => void): Promise<Unlisten> {
    return this.ipc.listen(JOBS_EVENT, (payload) => {
      const job = parseJobSnapshot(payload);
      if (!job || !job.kind.startsWith('storage-')) return;
      if (job.state === 'running') {
        const phase = job.phase;
        handler(phase ? { label: phase.label, done: phase.done, total: phase.total ?? null } : { label: job.title, done: 0, total: null });
      } else if (job.state !== 'queued') {
        handler(null);
      }
    });
  }
}

/** What the in-memory engine does. */
export interface MemoryStorageOptions {
  /** The categories at the start (default: a small sample of a cache, logs and the user's models). */
  readonly categories?: readonly StorageCategory[];
  /** Runs the engine's next step; the default runs it at once. A test queues steps and runs them one at a time to see each phase. */
  readonly schedule?: (step: () => void) => void;
  /** Starts with this usage already measured (as if the app measured at start). */
  readonly measured?: boolean;
}

/** A sample of what an app keeps: a cache, its logs (today's stays) and the user's models. Sizes are fictional. */
export function sampleCategories(): StorageCategory[] {
  const item = (name: string, bytes: number, protectedBy?: string) => ({ name, bytes, ...(protectedBy ? { protected: protectedBy } : {}) });
  return [
    { id: 'cache', label: 'Cache', kind: 'cache', path: '/demo/app/cache', exists: true, bytes: 48_000_000, files: 312, clearableBytes: 48_000_000, clearableItems: 3, capped: false, clearable: true, note: 'Rebuilt when the app needs it.', items: [item('thumbnails', 30_000_000), item('previews', 18_000_000)] },
    { id: 'logs', label: 'Logs', kind: 'data', path: '/demo/app/logs', exists: true, bytes: 6_500_000, files: 9, clearableBytes: 5_000_000, clearableItems: 8, capped: false, clearable: true, note: "Today's log is kept.", items: [item('app.2026-10-23.log', 3_000_000), item('app.2026-10-24.log', 1_500_000, 'being written')] },
    { id: 'models', label: 'Models', kind: 'data', path: '/demo/app/models', exists: true, bytes: 4_200_000_000, files: 2, clearableBytes: 2_100_000_000, clearableItems: 1, capped: true, clearable: true, note: 'The active model is kept.', items: [item('small.gguf', 2_100_000_000), item('large.gguf', 2_100_000_000, 'the active model')] },
  ];
}

/**
 * The storage engine in memory, for the browser build and tests. It keeps the rules of the Rust side: a clear needs a scan first
 * (and uses it up), a `data` category needs its confirmation, a second job while one runs is `busy`, Cancel stops between items;
 * and it plays the phases of a job. It never touches a real file.
 */
export class MemoryStorageBackend extends StorageBackend {
  /** Every call, in order, as `[command, args]`. */
  readonly calls: [StorageCommand, Record<string, unknown> | undefined][] = [];
  /** The category ids shown in the file manager. */
  readonly revealed: string[] = [];
  private categories: StorageCategory[];
  private usage: StorageUsage | null;
  private previewed = false;
  private job: 'scan' | 'clear' | null = null;
  private cancelled = false;
  private readonly schedule: (step: () => void) => void;
  private readonly events = new Set<(e: StorageEvent) => void>();
  private readonly progress = new Set<(p: StorageProgress | null) => void>();

  constructor(options: MemoryStorageOptions = {}) {
    super();
    this.categories = (options.categories ?? sampleCategories()).map((c) => ({ ...c }));
    this.schedule = options.schedule ?? ((step) => step());
    this.usage = options.measured ? this.measure() : null;
    this.previewed = options.measured === true;
  }

  private measure(): StorageUsage {
    return { categories: this.categories.map((c) => ({ ...c })), totalBytes: this.categories.reduce((n, c) => n + c.bytes, 0), capped: this.categories.some((c) => c.capped) };
  }

  private call(command: StorageCommand, args?: Record<string, unknown>): void {
    this.calls.push([command, args]);
  }

  private refuse(code: CommandError['code'], message: string, kind?: string): never {
    throw { code, message, retryable: code === 'busy', ...(kind ? { kind } : {}) } satisfies CommandError;
  }

  /** The engine sends an outcome on its own (tests: a job of another window ends, a job fails). */
  send(event: StorageEvent): void {
    for (const h of [...this.events]) h(event);
  }

  /** The engine reports a phase on its own (tests: another window's job runs). */
  report(p: StorageProgress | null): void {
    for (const h of [...this.progress]) h(p);
  }

  last(): Promise<StorageUsage | null> {
    return this.run(() => {
      this.call('storage_last');
      return this.usage;
    });
  }

  scan(): Promise<void> {
    return this.run(() => {
      this.call('storage_scan');
      this.begin('scan');
      const steps = this.categories.map((c, i) => () => {
        if (this.cancelled) return;
        this.report({ label: `Measuring ${c.label}`, done: i, total: this.categories.length });
      });
      this.play(steps, () => {
        if (this.cancelled) return this.end({ state: 'cancelled' });
        this.usage = this.measure();
        this.previewed = true;
        this.end({ state: 'usage', usage: this.usage });
      });
    });
  }

  clear(request: ClearRequest): Promise<void> {
    return this.run(() => {
      this.call('storage_clear', { request });
      const ids = request.categories.map((c) => c.id);
      if (ids.length > 32) this.refuse('invalid', 'Too many categories in one request.');
      const dup = ids.find((id, i) => ids.indexOf(id) !== i);
      if (dup) this.refuse('invalid', `"${dup}" is listed twice.`);
      const unknown = ids.find((id) => !this.categories.some((c) => c.id === id));
      if (unknown) this.refuse('notFound', `"${unknown}" is not something this app keeps on disk.`);
      if (!this.previewed) this.refuse('invalid', 'Measure the storage first, so you can see what would be removed.', 'noPreview');
      this.begin('clear');
      this.previewed = false;
      const results: CategoryClearResult[] = [];
      const steps = request.categories.map((choice, i) => () => {
        if (this.cancelled) return;
        const c = this.categories.find((x) => x.id === choice.id)!;
        this.report({ label: 'Removing', done: i, total: request.categories.length });
        if (c.kind === 'data' && !choice.confirmed) {
          results.push({ id: c.id, freedBytes: 0, removedFiles: 0, skipped: [], skippedCount: 0, error: 'It holds your own data: confirm to clear it.' });
          return;
        }
        const freed = c.clearableBytes;
        results.push({ id: c.id, freedBytes: freed, removedFiles: c.clearableItems, skipped: [], skippedCount: 0 });
        c.bytes -= freed;
        c.clearableBytes = 0;
        c.clearableItems = 0;
        c.items = c.items.filter((it) => it.protected);
      });
      this.play(steps, () => {
        const result: StorageClearResult = { freedBytes: results.reduce((n, r) => n + r.freedBytes, 0), categories: results, cancelled: this.cancelled };
        this.usage = null;
        this.end({ state: 'cleared', result });
      });
    });
  }

  cancel(): Promise<boolean> {
    return this.run(() => {
      this.call('storage_cancel');
      if (!this.job) return false;
      this.cancelled = true;
      return true;
    });
  }

  reveal(id: string): Promise<void> {
    return this.run(() => {
      this.call('storage_reveal', { id });
      const c = this.categories.find((x) => x.id === id);
      if (!c || !c.exists) this.refuse('notFound', `"${id}" is not something this app keeps on disk.`);
      this.revealed.push(id);
    });
  }

  listen(handler: (event: StorageEvent) => void): Promise<Unlisten> {
    this.events.add(handler);
    return Promise.resolve(() => void this.events.delete(handler));
  }

  listenProgress(handler: (progress: StorageProgress | null) => void): Promise<Unlisten> {
    this.progress.add(handler);
    return Promise.resolve(() => void this.progress.delete(handler));
  }

  private begin(job: 'scan' | 'clear'): void {
    if (this.job) this.refuse('busy', 'Storage is being measured or cleared already');
    this.job = job;
    this.cancelled = false;
  }

  /** Plays the steps through the scheduler, then finishes. */
  private play(steps: (() => void)[], finish: () => void): void {
    const all = [...steps, finish];
    const next = (): void => {
      const step = all.shift();
      if (!step) return;
      step();
      if (all.length > 0 && this.job) this.schedule(next);
    };
    this.schedule(next);
  }

  private end(event: StorageEvent): void {
    this.job = null;
    this.report(null);
    this.send(event);
  }

  private run<T>(step: () => T): Promise<T> {
    try {
      return Promise.resolve(step());
    } catch (e) {
      return Promise.reject(toCommandError(e));
    }
  }
}
