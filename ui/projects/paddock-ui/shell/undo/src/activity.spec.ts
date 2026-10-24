import { TestBed } from '@angular/core/testing';
import { describe, expect, it, vi } from 'vitest';
import { ACTIVITY_PAGE_SIZE, ActivityBackend, MemoryActivityBackend, TauriActivityBackend, type ActivityIpc, type Unlisten } from './activity-backend';
import { PdActivityPage } from './activity-page';
import { ACTIVITY_EVENT, parseActivityEntries, parseActivityEntry, type ActivityEntry } from './activity-wire';
import { ACTIVITY_CONTRIBUTIONS } from './activity.contributions';
import { ACTIVITY_NOTICE_KIND, ActivityStore, mergeEntry } from './activity.store';
import { provideShellActivity } from './provide-activity';
import { bed, settle } from './spec-helpers';

const entry = (id: number, extra: Partial<ActivityEntry> = {}): ActivityEntry => ({
  id,
  atMs: Date.UTC(2026, 9, 22, 8, id),
  kind: 'rule',
  title: `Action ${id}`,
  detail: '',
  undo: 'available',
  bytes: null,
  ...extra,
});

describe('activity wire', () => {
  it('reads an entry and drops anything else', () => {
    expect(parseActivityEntry(entry(1, { bytes: 5 }))).toEqual(entry(1, { bytes: 5 }));
    expect(parseActivityEntry({ ...entry(1), bytes: undefined })?.bytes).toBeNull();
    const bad: unknown[] = [
      null,
      'x',
      {},
      { ...entry(1), id: -1 },
      { ...entry(1), id: 1.5 },
      { ...entry(1), atMs: Number.NaN },
      { ...entry(1), kind: 3 },
      { ...entry(1), title: null },
      { ...entry(1), detail: 1 },
      { ...entry(1), undo: 'maybe' },
      { ...entry(1), bytes: -1 },
      { ...entry(1), bytes: 'many' },
    ];
    for (const b of bad) expect(parseActivityEntry(b), JSON.stringify(b)).toBeNull();
    expect(parseActivityEntries([entry(1), { nope: 1 }, entry(2)]).map((e) => e.id)).toEqual([1, 2]);
    expect(parseActivityEntries('x')).toEqual([]);
  });

  it('merges a changed entry in place, a new one on top, and keeps the newest ones', () => {
    const list = [entry(3), entry(2), entry(1)];
    expect(mergeEntry(list, entry(2, { undo: 'undone' })).map((e) => [e.id, e.undo])).toEqual([[3, 'available'], [2, 'undone'], [1, 'available']]);
    expect(mergeEntry(list, entry(4)).map((e) => e.id)).toEqual([4, 3, 2, 1]);
    expect(mergeEntry(list, entry(4), 2).map((e) => e.id)).toEqual([4, 3]);
  });
});

describe('Tauri activity backend', () => {
  function ipc(answers: Record<string, unknown>) {
    const handlers: ((p: unknown) => void)[] = [];
    const calls: [string, Record<string, unknown> | undefined][] = [];
    const unlisten = vi.fn();
    const impl: ActivityIpc = {
      invoke: <T>(command: string, args?: Record<string, unknown>) => {
        calls.push([command, args]);
        return Promise.resolve(answers[command] as T);
      },
      listen: (event: string, handler: (p: unknown) => void) => {
        expect(event).toBe(ACTIVITY_EVENT);
        handlers.push(handler);
        return Promise.resolve(unlisten as Unlisten);
      },
    };
    return { impl, calls, handlers, unlisten };
  }

  it('lists, undoes and listens through the commands, parsing at the edge', async () => {
    const t = ipc({ activity_list: [entry(2), { bad: true }], activity_undo: entry(2, { undo: 'undone' }) });
    const backend = new TauriActivityBackend(t.impl);
    expect((await backend.list(50)).map((e) => e.id)).toEqual([2]);
    expect((await backend.undo(2)).undo).toBe('undone');
    expect(t.calls).toEqual([['activity_list', { limit: 50 }], ['activity_undo', { id: 2 }]]);
    const heard: number[] = [];
    await backend.listen((e) => heard.push(e.id));
    t.handlers[0]!(entry(7));
    t.handlers[0]!({ nope: 1 });
    expect(heard).toEqual([7]);
  });

  it('uses the app’s command names and refuses an answer that is not an entry', async () => {
    const t = ipc({ my_list: [], my_undo: 'nothing' });
    const backend = new TauriActivityBackend(t.impl, { list: 'my_list', undo: 'my_undo' });
    await backend.list(1);
    await expect(backend.undo(1)).rejects.toThrow('did not answer with the entry');
    expect(t.calls.map((c) => c[0])).toEqual(['my_list', 'my_undo']);
  });
});

describe('Memory activity backend', () => {
  it('records, lists newest first, undoes once and tells the listeners', async () => {
    const undone: number[] = [];
    const backend = new MemoryActivityBackend((e) => void undone.push(e.id));
    const heard: ActivityEntry[] = [];
    const off = await backend.listen((e) => heard.push(e));
    const a = backend.record({ title: 'a', undo: 'available' });
    backend.record({ title: 'b' });
    expect((await backend.list(10)).map((e) => e.title)).toEqual(['b', 'a']);
    expect((await backend.list(1)).length).toBe(1);
    expect((await backend.undo(a.id)).undo).toBe('undone');
    expect(undone).toEqual([a.id]);
    expect(heard.map((e) => e.undo)).toEqual(['available', 'notUndoable', 'undone']);
    await expect(backend.undo(a.id)).rejects.toThrow('cannot be undone');
    await expect(backend.undo(99)).rejects.toThrow('no longer in the log');
    off();
    backend.record({ title: 'c' });
    expect(heard).toHaveLength(3);
  });

  it('passes on a refusal of the app’s undo', async () => {
    const backend = new MemoryActivityBackend(() => {
      throw new Error('the process has exited');
    });
    const e = backend.record({ title: 'a', undo: 'available' });
    await expect(backend.undo(e.id)).rejects.toThrow('the process has exited');
    const odd = new MemoryActivityBackend(() => {
      throw 'plain text';
    });
    const o = odd.record({ title: 'a', undo: 'available' });
    await expect(odd.undo(o.id)).rejects.toThrow('plain text');
  });
});

/** A backend whose list and listen answer when the test says so. */
class ControlledBackend extends ActivityBackend {
  listResult: { resolve: (v: readonly ActivityEntry[]) => void; reject: (e: unknown) => void } | null = null;
  handler: ((e: ActivityEntry) => void) | null = null;
  unlisten = vi.fn();
  listenResult: Promise<Unlisten> = Promise.resolve(this.unlisten);
  undoResult: Promise<ActivityEntry> = Promise.resolve(entry(1, { undo: 'undone' }));
  list(): Promise<readonly ActivityEntry[]> {
    return new Promise((resolve, reject) => (this.listResult = { resolve, reject }));
  }
  undo(): Promise<ActivityEntry> {
    return this.undoResult;
  }
  listen(handler: (e: ActivityEntry) => void): Promise<Unlisten> {
    this.handler = handler;
    return this.listenResult;
  }
}

describe('ActivityStore', () => {
  function setup(backend: ActivityBackend) {
    const b = bed([provideShellActivity({ backend: () => backend })]);
    return { ...b, store: TestBed.inject(ActivityStore) };
  }

  it('loads: loading, then loaded; an event during the load is kept', async () => {
    const backend = new ControlledBackend();
    const { store } = setup(backend);
    expect(store.requestStatus()).toBe('loading');
    await settle();
    backend.handler!(entry(5));
    backend.listResult!.resolve([entry(2), entry(1)]);
    await settle();
    expect(store.requestStatus()).toBe('loaded');
    expect(store.entries().map((e) => e.id)).toEqual([5, 2, 1]);
    expect(store.undoableCount()).toBe(3);
    backend.handler!(entry(2, { undo: 'undone' }));
    expect(store.undoableCount()).toBe(2);
  });

  it('shows a failed load as an error that Retry clears', async () => {
    const backend = new ControlledBackend();
    const { store } = setup(backend);
    backend.listResult!.reject(new Error('the log is locked'));
    await settle();
    expect(store.requestError()).toBe('the log is locked');
    const retry = store.load();
    expect(store.isLoading()).toBe(true);
    backend.listResult!.resolve([entry(1)]);
    await retry;
    expect(store.isLoaded()).toBe(true);
    const again = store.load();
    backend.listResult!.reject({});
    await again;
    expect(store.requestError()).toBe('The activity log did not answer.');
  });

  it('undoes an entry once at a time and shows the new state', async () => {
    const backend = new ControlledBackend();
    const { store } = setup(backend);
    backend.listResult!.resolve([entry(1)]);
    await settle();
    let release: (e: ActivityEntry) => void = () => undefined;
    backend.undoResult = new Promise((resolve) => (release = resolve));
    const first = store.undo(1);
    expect(store.undoing()).toEqual([1]);
    expect(await store.undo(1)).toBe(false);
    release(entry(1, { undo: 'undone' }));
    expect(await first).toBe(true);
    expect(store.undoing()).toEqual([]);
    expect(store.entries()[0]!.undo).toBe('undone');
  });

  it('turns a refused undo into a notice with the app’s reason and leaves the entry', async () => {
    const backend = new ControlledBackend();
    const { store, notices } = setup(backend);
    backend.listResult!.resolve([entry(1)]);
    await settle();
    backend.undoResult = Promise.reject(new Error('Chrome has exited'));
    expect(await store.undo(1)).toBe(false);
    backend.undoResult = Promise.reject({});
    expect(await store.undo(1)).toBe(false);
    expect(notices).toEqual([
      { level: 'error', kind: ACTIVITY_NOTICE_KIND, message: 'Chrome has exited' },
      { level: 'error', kind: ACTIVITY_NOTICE_KIND, message: 'The action could not be undone.' },
    ]);
    expect(store.entries()[0]!.undo).toBe('available');
    expect(store.undoing()).toEqual([]);
  });

  it('stops listening when destroyed, also when the listener was still being set up', async () => {
    const quick = new ControlledBackend();
    setup(quick);
    await settle();
    TestBed.resetTestingModule();
    expect(quick.unlisten).toHaveBeenCalledTimes(1);

    const slow = new ControlledBackend();
    let attach: (u: Unlisten) => void = () => undefined;
    slow.listenResult = new Promise((resolve) => (attach = resolve));
    setup(slow);
    TestBed.resetTestingModule();
    attach(slow.unlisten);
    await settle();
    expect(slow.unlisten).toHaveBeenCalledTimes(1);
  });

  it('warns when it cannot listen and still loads', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    const backend = new ControlledBackend();
    backend.listenResult = Promise.reject(new Error('no events'));
    const { store } = setup(backend);
    backend.listResult!.resolve([entry(1)]);
    await settle();
    expect(warn).toHaveBeenCalledWith(expect.stringContaining('no events'));
    expect(store.entries()).toHaveLength(1);
    warn.mockRestore();
  });

  it('asks for a page of the newest entries', async () => {
    const backend = new MemoryActivityBackend();
    const spy = vi.spyOn(backend, 'list');
    setup(backend);
    await settle();
    expect(spy).toHaveBeenCalledWith(ACTIVITY_PAGE_SIZE);
  });
});

describe('pd-activity-page', () => {
  function mount(backend: ActivityBackend) {
    const b = bed([provideShellActivity({ backend: () => backend })]);
    const f = TestBed.createComponent(PdActivityPage);
    const el: HTMLElement = f.nativeElement;
    return { ...b, f, el, rows: () => [...el.querySelectorAll<HTMLElement>('tbody tr')] };
  }

  it('lists the entries with time, what, size and the undo state of each', async () => {
    const backend = new MemoryActivityBackend();
    backend.record({ title: 'Lowered Chrome’s priority', detail: 'Rule: background apps', undo: 'available', atMs: Date.UTC(2026, 9, 22, 8, 0) });
    backend.record({ title: 'Moved 3 files', undo: 'undone', bytes: 2048 });
    backend.record({ title: 'Quarantine ran out', undo: 'expired' });
    backend.record({ title: 'Started', undo: 'notUndoable' });
    const t = mount(backend);
    await settle();
    await t.f.whenStable();
    const rows = t.rows();
    expect(rows.map((r) => r.querySelector('.pd-title')!.textContent)).toEqual(['Started', 'Quarantine ran out', 'Moved 3 files', 'Lowered Chrome’s priority']);
    expect(rows[3]!.querySelector('.pd-detail')!.textContent).toBe('Rule: background apps');
    expect(rows[3]!.querySelector('button')!.getAttribute('aria-label')).toBe('Undo: Lowered Chrome’s priority');
    expect(rows[2]!.querySelector('.pd-size')!.textContent).toBe('2 KB');
    expect(rows[2]!.querySelector('.pd-state')!.textContent).toBe('Undone');
    expect(rows[1]!.querySelector('.pd-state')!.textContent).toBe('No longer possible');
    expect(rows[0]!.querySelector('button, .pd-state')).toBeNull();
    expect(t.el.querySelector('h1')!.textContent).toBe('Activity');
  });

  it('undoes an entry from its row and then shows it as undone', async () => {
    const backend = new MemoryActivityBackend();
    backend.record({ title: 'Lowered priority', undo: 'available' });
    const t = mount(backend);
    await settle();
    await t.f.whenStable();
    t.rows()[0]!.querySelector<HTMLButtonElement>('button')!.click();
    await settle();
    await t.f.whenStable();
    TestBed.tick();
    expect(t.rows()[0]!.querySelector('.pd-state')!.textContent).toBe('Undone');
    expect(t.rows()[0]!.querySelector('button')).toBeNull();
  });

  it('shows an empty state, a loading skeleton and an error with Retry', async () => {
    const empty = mount(new MemoryActivityBackend());
    await settle();
    await empty.f.whenStable();
    expect(empty.el.querySelector('pd-empty-state')!.textContent).toContain('Nothing yet');
    TestBed.resetTestingModule();

    const backend = new ControlledBackend();
    const t = mount(backend);
    await settle();
    expect(t.el.querySelector('[aria-busy=true]')).not.toBeNull();
    backend.listResult!.reject(new Error('locked'));
    await settle();
    await t.f.whenStable();
    expect(t.el.querySelector('pd-error-state')!.textContent).toContain('locked');
    expect(t.el.querySelector('pd-empty-state')).toBeNull();
  });
});

describe('the Activity page registration', () => {
  it('is a lazy sidebar page', () => {
    const [page] = ACTIVITY_CONTRIBUTIONS.pages!;
    expect(page).toMatchObject({ id: 'pd.activity', icon: 'pd:activity', section: 'top' });
    expect(typeof page!.load).toBe('function');
  });
});
