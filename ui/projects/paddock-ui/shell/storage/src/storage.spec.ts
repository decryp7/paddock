import { provideZonelessChangeDetection } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { Injector } from '@angular/core';
import { assertContributionsValid } from '@decryp7/paddock-ui/shell/testing';
import { provideContributions } from '@decryp7/paddock-ui/shell';
import { JOBS_EVENT } from '@decryp7/paddock-ui/shell/jobs';
import { STORAGE_EVENT, type SizeNode, type StorageEvent, type StorageUsage } from '../generated/storage';
import { STORAGE_COMMANDS, type StorageCommand } from '../generated/storage-commands';
import { MemoryStorageBackend, StorageBackend, TauriStorageBackend, sampleCategories, sampleTree, type StorageIpc, type StorageProgress } from './backend';
import { provideShellStorage } from './provide-storage';
import { STORAGE_CONFIRM_DIALOG_ID, STORAGE_SETTINGS_COMMANDS, STORAGE_SETTINGS_PAGE, defineStorageConfirmDialog, defineStorageSettingsPage, storageContributions } from './storage.contributions';
import { StorageStore } from './storage.store';
import { treeRows } from './tree-rows';
import { clearRequest, parseStorageEvent, parseStorageTree, parseStorageUsage } from './wire';

/** Lets the simulated engine and the stores settle (microtasks only). */
async function settle(rounds = 12): Promise<void> {
  for (let i = 0; i < rounds; i++) await Promise.resolve();
}

/** The engine's steps wait until `next()`/`all()`: a test sees each phase. */
function held(options: ConstructorParameters<typeof MemoryStorageBackend>[0] = {}) {
  const queue: (() => void)[] = [];
  const memory = new MemoryStorageBackend({ schedule: (step) => queue.push(step), ...options });
  return {
    memory,
    pending: () => queue.length,
    async next(): Promise<void> {
      queue.shift()?.();
      await settle();
    },
    async all(): Promise<void> {
      while (queue.length) queue.shift()?.();
      await settle();
    },
  };
}

function bed(backend: StorageBackend) {
  TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection(), provideShellStorage({ backend: () => backend })] });
  return TestBed.inject(StorageStore);
}

const USAGE: StorageUsage = { categories: sampleCategories(), totalBytes: 123, capped: true };

describe('the wire', () => {
  it('rebuilds usage from known fields and drops what is not the shape', () => {
    expect(parseStorageUsage({ ...USAGE, extra: 1 })).toEqual(USAGE);
    const row = USAGE.categories[0]!;
    expect(parseStorageUsage({ ...USAGE, categories: [{ ...row, x: 1, path: undefined, note: undefined }] })?.categories[0]).toEqual({ ...row, path: undefined, note: undefined });
    for (const bad of [null, [], {}, { ...USAGE, totalBytes: -1 }, { ...USAGE, capped: 'no' }, { ...USAGE, categories: [{ ...row, kind: 'other' }] }, { ...USAGE, categories: [{ ...row, bytes: Number.NaN }] }, { ...USAGE, categories: [{ ...row, items: 5 }] }, { ...USAGE, categories: [{ ...row, path: 5 }] }]) {
      expect(parseStorageUsage(bad)).toBeNull();
    }
    expect(parseStorageUsage({ ...USAGE, categories: [{ ...row, items: [5, { name: 'a', bytes: 1, protected: 'x' }, { name: 1 }] }] })?.categories[0]?.items).toEqual([{ name: 'a', bytes: 1, protected: 'x' }]);
  });

  it('parses every outcome and refuses the rest', () => {
    const result = { freedBytes: 5, cancelled: false, categories: [{ id: 'cache', freedBytes: 5, removedFiles: 1, skippedCount: 1, skipped: [{ path: '/a', reason: 'in use' }, 3], error: 'no' }] };
    expect(parseStorageEvent({ state: 'usage', usage: USAGE })?.state).toBe('usage');
    expect(parseStorageEvent({ state: 'cleared', result })).toEqual({ state: 'cleared', result: { ...result, categories: [{ ...result.categories[0], skipped: [{ path: '/a', reason: 'in use' }] }] } });
    expect(parseStorageEvent({ state: 'cancelled' })).toEqual({ state: 'cancelled' });
    expect(parseStorageEvent({ state: 'failed', message: 'x' })).toEqual({ state: 'failed', message: 'x' });
    for (const bad of [null, 'x', { state: 'usage' }, { state: 'cleared', result: { freedBytes: 1 } }, { state: 'cleared', result: { ...result, categories: [null] } }, { state: 'failed' }, { state: 'nope' }]) {
      expect(parseStorageEvent(bad)).toBeNull();
    }
    expect(clearRequest([{ id: 'a', confirmed: true, extra: 1 } as { id: string; confirmed: boolean }])).toEqual({ categories: [{ id: 'a', confirmed: true }] });
  });

  it('names every command of the table', () => {
    expect(Object.keys(STORAGE_COMMANDS).sort()).toEqual(['storage_cancel', 'storage_clear', 'storage_last', 'storage_reveal', 'storage_scan', 'storage_tree']);
    expect([...STORAGE_SETTINGS_COMMANDS].sort()).toEqual(Object.keys(STORAGE_COMMANDS).sort());
  });
});

describe('the Tauri backend', () => {
  function ipc(replies: Partial<Record<StorageCommand, unknown>>) {
    const calls: [string, unknown][] = [];
    const handlers = new Map<string, (p: unknown) => void>();
    const fake: StorageIpc = {
      invoke: <T>(command: StorageCommand, args?: Record<string, unknown>) => {
        calls.push([command, args]);
        return Promise.resolve(replies[command] as T);
      },
      listen: (event, handler) => {
        handlers.set(event, handler);
        return Promise.resolve(() => void handlers.delete(event));
      },
    };
    return { fake, calls, handlers };
  }

  it('sends every request with its arguments', async () => {
    const { fake, calls } = ipc({ storage_last: USAGE, storage_cancel: true });
    const b = new TauriStorageBackend(fake);
    expect(await b.last()).toEqual(USAGE);
    await b.scan();
    await b.clear({ categories: [{ id: 'cache', confirmed: false }] });
    expect(await b.cancel()).toBe(true);
    await b.reveal('logs');
    expect(calls).toEqual([['storage_last', undefined], ['storage_scan', undefined], ['storage_clear', { request: { categories: [{ id: 'cache', confirmed: false }] } }], ['storage_cancel', undefined], ['storage_reveal', { id: 'logs' }]]);
    expect(await new TauriStorageBackend(ipc({ storage_last: null }).fake).last()).toBeNull();
    expect(await new TauriStorageBackend(ipc({ storage_cancel: 0 }).fake).cancel()).toBe(false);
  });

  it('refuses figures that are not the contract', async () => {
    await expect(new TauriStorageBackend(ipc({ storage_last: { nope: 1 } }).fake).last()).rejects.toThrow('no valid storage figures');
  });

  it('listens to the outcome and to the job events of storage only', async () => {
    const { fake, handlers } = ipc({});
    const b = new TauriStorageBackend(fake);
    const events: StorageEvent[] = [];
    const progress: (StorageProgress | null)[] = [];
    const offEvents = await b.listen((e) => events.push(e));
    const offProgress = await b.listenProgress((p) => progress.push(p));
    handlers.get(STORAGE_EVENT)?.({ state: 'cancelled' });
    handlers.get(STORAGE_EVENT)?.('junk');
    const base = { id: 1, kind: 'storage-clear', title: 'Clearing storage', subject: null, seq: 1, elapsedMs: 0, etaMs: null, cancellable: true, notCancellableReason: null };
    const jobs = handlers.get(JOBS_EVENT)!;
    jobs({ ...base, state: 'running', phase: { id: 'clear', label: 'Removing', unit: 'items', total: 3, done: 1 } });
    jobs({ ...base, state: 'running', phase: { id: 'scan', label: 'Counting', unit: 'items', total: null, done: 7 } });
    jobs({ ...base, state: 'running', phase: null });
    jobs({ ...base, state: 'queued' });
    jobs({ ...base, kind: 'download', state: 'running', phase: { id: 'd', label: 'Other', unit: 'bytes', total: 1, done: 0 } });
    jobs({ ...base, state: 'done' });
    jobs('junk');
    expect(events).toEqual([{ state: 'cancelled' }]);
    expect(progress).toEqual([{ label: 'Removing', done: 1, total: 3 }, { label: 'Counting', done: 7, total: null }, { label: 'Clearing storage', done: 0, total: null }, null]);
    offEvents();
    offProgress();
    expect(handlers.size).toBe(0);
  });
});

describe('the in-memory engine keeps the rules of the Rust side', () => {
  it('needs a scan before a clear, and a clear uses the scan up', async () => {
    const h = held();
    await expect(h.memory.clear({ categories: [{ id: 'cache', confirmed: false }] })).rejects.toMatchObject({ code: 'invalid', kind: 'noPreview' });
    expect(await h.memory.last()).toBeNull();
    await h.memory.scan();
    await h.all();
    expect((await h.memory.last())?.totalBytes).toBe(sampleCategories().reduce((n, c) => n + c.bytes, 0));
    await h.memory.clear({ categories: [{ id: 'cache', confirmed: false }] });
    await h.all();
    await expect(h.memory.clear({ categories: [{ id: 'cache', confirmed: false }] })).rejects.toMatchObject({ kind: 'noPreview' });
    expect(await h.memory.last()).toBeNull();
  });

  it('refuses what the engine refuses, before a job exists', async () => {
    const h = held({ measured: true });
    await expect(h.memory.clear({ categories: [{ id: 'nope', confirmed: true }] })).rejects.toMatchObject({ code: 'notFound' });
    await expect(h.memory.clear({ categories: [{ id: 'cache', confirmed: true }, { id: 'cache', confirmed: true }] })).rejects.toMatchObject({ code: 'invalid' });
    await expect(h.memory.clear({ categories: Array.from({ length: 33 }, (_, i) => ({ id: `c${i}`, confirmed: false })) })).rejects.toMatchObject({ code: 'invalid' });
    expect(h.pending()).toBe(0);
    await expect(h.memory.reveal('nope')).rejects.toMatchObject({ code: 'notFound' });
    await h.memory.reveal('cache');
    expect(h.memory.revealed).toEqual(['cache']);
  });

  it('runs one job at a time, plays its phases and ends with an event', async () => {
    const h = held();
    const events: StorageEvent[] = [];
    const phases: (StorageProgress | null)[] = [];
    await h.memory.listen((e) => events.push(e));
    await h.memory.listenProgress((p) => phases.push(p));
    await h.memory.scan();
    await expect(h.memory.scan()).rejects.toMatchObject({ code: 'busy', retryable: true });
    await h.next();
    expect(phases.at(-1)).toEqual({ label: 'Measuring Cache', done: 0, total: 3 });
    await h.all();
    expect(events.map((e) => e.state)).toEqual(['usage']);
    expect(phases.at(-1)).toBeNull();
  });

  it('cancels a scan without changes and a clear part way', async () => {
    const h = held();
    const events: StorageEvent[] = [];
    await h.memory.listen((e) => events.push(e));
    expect(await h.memory.cancel()).toBe(false);
    await h.memory.scan();
    expect(await h.memory.cancel()).toBe(true);
    await h.all();
    expect(events.map((e) => e.state)).toEqual(['cancelled']);
    await h.memory.scan();
    await h.all();
    await h.memory.clear({ categories: [{ id: 'cache', confirmed: false }, { id: 'logs', confirmed: true }] });
    await h.next();
    await h.memory.cancel();
    await h.all();
    const last = events.at(-1);
    expect(last?.state === 'cleared' && last.result.cancelled).toBe(true);
    expect(last?.state === 'cleared' && last.result.categories.map((c) => c.id)).toEqual(['cache']);
  });

  it('asks for the confirmation of the users data and keeps what must stay', async () => {
    const h = held({ measured: true });
    const events: StorageEvent[] = [];
    await h.memory.listen((e) => events.push(e));
    await h.memory.clear({ categories: [{ id: 'logs', confirmed: false }, { id: 'cache', confirmed: false }] });
    await h.all();
    const done = events.at(-1);
    if (done?.state !== 'cleared') throw new Error('cleared');
    expect(done.result.categories[0]?.error).toContain('confirm');
    expect(done.result.freedBytes).toBe(48_000_000);
  });
});

describe('the store', () => {
  it('measures at the start when the engine has no figures, and shows them when it has', async () => {
    const h = held();
    const store = bed(h.memory);
    await settle();
    expect(store.working()).toBe('scan');
    await h.all();
    expect(store.working()).toBeNull();
    expect(store.requestStatus()).toBe('loaded');
    expect(store.categories()).toHaveLength(3);
    expect(store.cache()).toEqual({ ids: ['cache'], bytes: 48_000_000 });

    TestBed.resetTestingModule();
    const known = held({ measured: true });
    const again = bed(known.memory);
    await settle();
    expect(again.working()).toBeNull();
    expect(known.pending()).toBe(0);
    expect(again.usage()?.categories).toHaveLength(3);
  });

  it('says "error" when it cannot ask, and Retry asks again', async () => {
    const h = held();
    let fail = true;
    const last = h.memory.last.bind(h.memory);
    h.memory.last = () => (fail ? Promise.reject(new Error('the engine is down')) : last());
    const store = bed(h.memory);
    await settle();
    expect(store.requestStatus()).toEqual({ error: 'the engine is down' });
    fail = false;
    await store.load();
    await h.all();
    expect(store.requestStatus()).toBe('loaded');
  });

  it('follows a clear: the phase, the result, and a new measurement so the figures are never stale', async () => {
    const h = held({ measured: true });
    const store = bed(h.memory);
    await settle();
    expect(await store.clear([{ id: 'cache', confirmed: false }])).toBe(true);
    expect(store.working()).toBe('clear');
    expect(await store.clear([{ id: 'cache', confirmed: false }])).toBe(false);
    await h.next();
    expect(store.progress()).toEqual({ label: 'Removing', done: 0, total: 1 });
    await h.next();
    expect(store.result()?.freedBytes).toBe(48_000_000);
    expect(store.working()).toBe('scan');
    await h.all();
    expect(store.working()).toBeNull();
    expect(store.cache().ids).toEqual([]);
    store.dismissResult();
    expect(store.result()).toBeNull();
  });

  it('keeps a refusal as an inline failure and Cancel only while something runs', async () => {
    const h = held({ measured: true });
    const store = bed(h.memory);
    await settle();
    await store.cancel();
    expect(store.cancelling()).toBe(false);
    expect(await store.clear([{ id: 'nope', confirmed: true }])).toBe(false);
    expect(store.failure()?.code).toBe('notFound');
    expect(store.working()).toBeNull();
    store.dismissFailure();
    await store.reveal('nope');
    expect(store.failure()?.code).toBe('notFound');
    await store.reveal('cache');
    expect(store.failure()).toBeNull();
    await store.scan();
    await store.cancel();
    expect(store.cancelling()).toBe(true);
    await h.all();
    expect(store.cancelling()).toBe(false);
    expect(store.working()).toBeNull();
    h.memory.cancel = () => Promise.reject(new Error('gone'));
    await store.scan();
    await store.cancel();
    expect(store.failure()?.message).toBe('gone');
  });

  it('shows a failed job as an inline failure', async () => {
    const h = held({ measured: true });
    const store = bed(h.memory);
    await settle();
    h.memory.send({ state: 'failed', message: 'The disk is not there.' });
    expect(store.failure()).toMatchObject({ message: 'The disk is not there.', retryable: true });
    h.memory.report({ label: 'Measuring Cache', done: 1, total: 2 });
    expect(store.working()).toBe('scan');
  });

  it('stops listening when it is destroyed, even if the listen answer arrives late', async () => {
    const h = held({ measured: true });
    const store = bed(h.memory);
    await settle();
    TestBed.resetTestingModule();
    expect(store.usage()).not.toBeNull();

    const late = held({ measured: true });
    let resolve: (off: () => void) => void = () => undefined;
    let offCalled = 0;
    late.memory.listen = () => new Promise((r) => (resolve = r));
    bed(late.memory);
    TestBed.resetTestingModule();
    resolve(() => void offCalled++);
    await settle();
    expect(offCalled).toBe(1);

    const failing = held({ measured: true });
    failing.memory.listenProgress = () => Promise.reject(new Error('no events'));
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    bed(failing.memory);
    await settle();
    expect(warn).toHaveBeenCalledWith(expect.stringContaining('no events'));
    warn.mockRestore();
  });
});

describe('the contributions', () => {
  it('declare a Storage page and a confirmation dialog that pass the registry lint', () => {
    const page = defineStorageSettingsPage({ providers: () => Promise.resolve([]), load: () => Promise.resolve(class {}) });
    const dialog = defineStorageConfirmDialog({ load: () => Promise.resolve(class {}) });
    expect(page.id).toBe(STORAGE_SETTINGS_PAGE);
    expect(dialog.id).toBe(STORAGE_CONFIRM_DIALOG_ID);
    expect(dialog.kind).toBe('decision');
    const batch = storageContributions(page, [dialog]);
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection(), provideContributions(batch)] });
    assertContributionsValid(TestBed.inject(Injector));
  });
});

describe('the size tree', () => {
  const node = (name: string, bytes: number, children: SizeNode[] = [], folded = 0) => ({ name, bytes, folded, children });
  const TREE = { id: 'cache', capped: false, root: node('Cache', 100, [node('thumbs', 60, [node('a', 40), node('b', 20)]), node('x.bin', 30), node('', 10, [], 4)]) };

  it('parses a tree from known fields, to a bounded depth, and refuses the rest', () => {
    expect(parseStorageTree({ ...TREE, extra: 1 })).toEqual(TREE);
    expect(parseStorageEvent({ state: 'tree', tree: TREE })).toEqual({ state: 'tree', tree: TREE });
    let deep: SizeNode = node('leaf', 1);
    for (let i = 0; i < 12; i++) deep = node(`d${i}`, 1, [deep]);
    for (const bad of [null, {}, { ...TREE, capped: 1 }, { ...TREE, root: null }, { ...TREE, root: node('r', -1) }, { ...TREE, root: { ...node('r', 1), children: [5] } }, { ...TREE, root: deep }, { state: 'tree' }]) {
      expect(parseStorageTree(bad) ?? parseStorageEvent(bad)).toBeNull();
    }
  });

  it('is flattened into rows grouped by top-level folder, with files and folds apart', () => {
    const rows = treeRows(TREE, { files: 'Files here', smaller: 'Smaller items', others: (n) => `${n} smaller items` });
    expect(rows.map((r) => [r.group, r.name, r.bytes, r.share])).toEqual([
      ['thumbs', 'thumbs', 60, 60],
      ['thumbs', 'a', 40, 40],
      ['thumbs', 'b', 20, 20],
      ['Files here', 'x.bin', 30, 30],
      ['Smaller items', '4 smaller items', 10, 10],
    ]);
    expect(new Set(rows.map((r) => r.key)).size).toBe(rows.length);
    expect(treeRows({ ...TREE, root: node('Empty', 0, [node('f', 0)]) }, { files: 'F', smaller: 'S', others: String })[0]?.share).toBe(0);
    const nested = treeRows({ id: 'c', capped: false, root: node('C', 10, [node('d', 10, [node('e', 10, [node('f', 10)])])]) }, { files: 'F', smaller: 'S', others: String });
    expect(nested.map((r) => r.name)).toEqual(['d', 'e', 'e/f']);
  });

  it('is walked as a job whose outcome is an event, and refused for an unknown or missing folder', async () => {
    const h = held({ measured: true });
    const events: StorageEvent[] = [];
    await h.memory.listen((e) => events.push(e));
    await h.memory.tree('cache');
    await expect(h.memory.tree('cache')).rejects.toMatchObject({ code: 'busy' });
    await h.all();
    const done = events.at(-1);
    expect(done?.state === 'tree' && done.tree.root.bytes).toBe(48_000_000);
    expect(sampleTree(sampleCategories()[0]!).children.at(-1)?.folded).toBe(7);
    await expect(h.memory.tree('nope')).rejects.toMatchObject({ code: 'notFound' });
    const missing = held({ categories: sampleCategories().map((c) => ({ ...c, exists: false })) });
    await expect(missing.memory.tree('cache')).rejects.toMatchObject({ code: 'unavailable' });
    const own = held({ measured: true, trees: { cache: node('Custom', 1) } });
    const seen: StorageEvent[] = [];
    await own.memory.listen((e) => seen.push(e));
    await own.memory.tree('cache');
    await own.memory.cancel();
    await own.all();
    expect(seen.map((e) => e.state)).toEqual(['cancelled']);
    await own.memory.tree('cache');
    await own.all();
    expect(seen.at(-1)).toMatchObject({ state: 'tree', tree: { root: { name: 'Custom' } } });
  });

  it('is asked through the store, kept per category and forgotten when the figures are measured again', async () => {
    const h = held({ measured: true });
    const store = bed(h.memory);
    await settle();
    expect(await store.tree('cache')).toBe(true);
    expect(store.working()).toBe('tree');
    await h.all();
    expect(store.trees()['cache']?.root.bytes).toBe(48_000_000);
    expect(await store.tree('nope')).toBe(false);
    expect(store.failure()?.code).toBe('notFound');
    await store.scan();
    await h.all();
    expect(store.trees()).toEqual({});
  });

  it('is sent to the engine by the Tauri backend', async () => {
    const calls: unknown[] = [];
    const b = new TauriStorageBackend({ invoke: (c, a) => { calls.push([c, a]); return Promise.resolve(undefined as never); }, listen: () => Promise.resolve(() => undefined) });
    await b.tree('logs');
    expect(calls).toEqual([['storage_tree', { id: 'logs' }]]);
  });
});
