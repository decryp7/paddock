import { storagePrefix } from './dock-options';
import {
  BrowserDockLayoutStore,
  type DockInvoke,
  InMemoryDockLayoutStore,
  IpcDockLayoutStore,
  LAYOUT_GET,
  LAYOUT_SET,
  LAYOUT_VALUE_MAX,
  layoutEntryProblem,
} from './dock-store';
import { parsePerspectiveState, serializePerspectiveState } from './perspectives';

describe('layoutEntryProblem', () => {
  it('accepts a key of the engine\'s shape and a JSON object', () => {
    expect(layoutEntryProblem('wb.layout', { a: 1 })).toBeNull();
    expect(layoutEntryProblem('a:b-c_d'.replace('_', '.'), {})).toBeNull();
  });

  it('refuses bad keys, non-objects and values over 1 MiB', () => {
    for (const key of ['', 'Upper', 'has space', 'x'.repeat(65), 'a/b']) expect(layoutEntryProblem(key, {}), key).toContain('key');
    for (const value of [null, [], 'x', 3]) expect(layoutEntryProblem('k', value)).toContain('object');
    expect(layoutEntryProblem('k', { big: 'x'.repeat(LAYOUT_VALUE_MAX) })).toContain('limited');
    const cyclic: Record<string, unknown> = {};
    cyclic['self'] = cyclic;
    expect(layoutEntryProblem('k', cyclic)).toContain('JSON');
  });
});

describe('InMemoryDockLayoutStore', () => {
  it('keeps documents and answers null for what it does not have, or cannot read', async () => {
    const store = new InMemoryDockLayoutStore();
    expect(await store.load('k')).toBeNull();
    await store.save('k', { a: 1 });
    expect(await store.load('k')).toEqual({ a: 1 });
    await store.save('Bad Key', { a: 1 });
    expect(store.entries.has('Bad Key')).toBe(false);
    store.entries.set('broken', '{nope');
    expect(await store.load('broken')).toBeNull();
  });
});

describe('BrowserDockLayoutStore', () => {
  afterEach(() => {
    vi.restoreAllMocks();
    localStorage.clear();
  });

  it('keeps documents in localStorage under its prefix', async () => {
    const store = new BrowserDockLayoutStore('t.');
    await store.save('k', { a: 1 });
    expect(localStorage.getItem('t.k')).toBe('{"a":1}');
    expect(await store.load('k')).toEqual({ a: 1 });
    expect(await store.load('other')).toBeNull();
    await store.save('Bad Key', { a: 1 });
    expect(localStorage.getItem('t.Bad Key')).toBeNull();
  });

  it('never throws when storage or the stored text is bad', async () => {
    localStorage.setItem('paddock.dock.k', '{nope');
    expect(await new BrowserDockLayoutStore().load('k')).toBeNull();
    vi.spyOn(Storage.prototype, 'getItem').mockImplementation(() => {
      throw new Error('denied');
    });
    vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => {
      throw new Error('full');
    });
    const store = new BrowserDockLayoutStore();
    expect(await store.load('k')).toBeNull();
    await expect(store.save('k', {})).resolves.toBeUndefined();
  });
});

describe('IpcDockLayoutStore', () => {
  const recorder = (answer: unknown = null, fail = false) => {
    const calls: [string, Record<string, unknown> | undefined][] = [];
    const invoke = (<T>(command: string, args?: Record<string, unknown>) => {
      calls.push([command, args]);
      return fail ? Promise.reject(new Error('away')) : Promise.resolve(answer as T);
    }) as DockInvoke;
    return { calls, store: new IpcDockLayoutStore(invoke) };
  };

  it('reads with layout_get { key } and writes with layout_set { key, value }', async () => {
    const r = recorder({ a: 1 });
    expect(await r.store.load('wb.layout')).toEqual({ a: 1 });
    await r.store.save('wb.layout', { b: 2 });
    expect(r.calls).toEqual([
      [LAYOUT_GET, { key: 'wb.layout' }],
      [LAYOUT_SET, { key: 'wb.layout', value: { b: 2 } }],
    ]);
  });

  it('answers null for nothing stored, never calls for a key or value the engine would refuse, and swallows failures', async () => {
    expect(await recorder(undefined).store.load('k')).toBeNull();
    const r = recorder();
    expect(await r.store.load('Bad Key')).toBeNull();
    await r.store.save('Bad Key', {});
    await r.store.save('k', [] as never);
    expect(r.calls).toEqual([]);
    const failing = recorder(null, true);
    expect(await failing.store.load('k')).toBeNull();
    await expect(failing.store.save('k', {})).resolves.toBeUndefined();
  });
});

describe('options and perspective state', () => {
  it('falls back to the default key prefix for an invalid one', () => {
    expect(storagePrefix({})).toBe('wb');
    expect(storagePrefix({ storageKey: 'app.main' })).toBe('app.main');
    expect(storagePrefix({ storageKey: 'Has Space' })).toBe('wb');
  });

  it('parses stored perspective state at the edge: junk gives the empty state', () => {
    for (const raw of [null, undefined, 'x', [], { version: 9 }]) expect(parsePerspectiveState(raw)).toEqual({ active: '', layouts: {} });
    const layout = { grid: { root: { type: 'leaf', data: { views: [], id: '1' } }, height: 1, width: 1, orientation: 'HORIZONTAL' }, panels: {} };
    const state = parsePerspectiveState({ version: 1, active: 'p', layouts: { p: { version: 1, layout, extras: {} }, bad: { version: 7 } } });
    expect(state.active).toBe('p');
    expect(Object.keys(state.layouts)).toEqual(['p']);
    expect(parsePerspectiveState({ version: 1, active: 5, layouts: [] }).active).toBe('');
    expect(parsePerspectiveState(JSON.parse(JSON.stringify(serializePerspectiveState(state))))).toEqual(state);
  });
});
