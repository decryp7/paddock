import { readValue } from '../core/measured';
import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../core/backend';
import { MockBackend } from '../core/mock-backend';
import type { LiveUpdate, ProcessDetails } from '../core/api.types';
import type { Row } from '../features/processes/process-rows';
import { Notifier } from '../shared/notifier';
import { ActivityStore } from './activity.store';
import { ProcessesStore } from './processes.store';

const NOW = 1_700_000_000_000;

function setup() {
  const mock = new MockBackend({ latencyMs: 0, now: () => NOW });
  TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: mock }] });
  const store = TestBed.inject(ProcessesStore);
  return { mock, store, notifier: TestBed.inject(Notifier), activity: TestBed.inject(ActivityStore) };
}
/** Starts the store against a backend whose onLive hands over `update` once, returning the pushed listener. */
function feed(mock: MockBackend, store: InstanceType<typeof ProcessesStore>) {
  let listener!: (u: LiveUpdate) => void;
  const stop = vi.fn();
  const spy = vi.spyOn(mock, 'onLive').mockImplementation((_d, l) => {
    listener = l;
    return stop;
  });
  store.start();
  listener(mock.liveUpdate('processes'));
  return { spy, stop, push: listener };
}

describe('ProcessesStore: subscription', () => {
  it('start subscribes once for the processes detail and fills the lists', () => {
    const { mock, store } = setup();
    const { spy } = feed(mock, store);
    store.start();
    expect(spy).toHaveBeenCalledTimes(1);
    expect(spy).toHaveBeenCalledWith('processes', expect.any(Function));
    expect(store.connected()).toBe(true);
    expect(store.total()).toBeGreaterThan(10);
    expect(store.groups().length).toBeGreaterThan(5);
    expect(store.rows().length).toBeGreaterThan(5);
  });
  it('ignores summary updates without a process list', () => {
    const { mock, store } = setup();
    const { push } = feed(mock, store);
    const before = store.total();
    push(mock.liveUpdate('summary'));
    expect(store.total()).toBe(before);
  });
  it('treats missing groups as empty', () => {
    const { mock, store } = setup();
    const { push } = feed(mock, store);
    push({ ...mock.liveUpdate('processes'), groups: null });
    expect(store.groups()).toEqual([]);
  });
  it('stop unsubscribes and allows starting again', () => {
    const { mock, store } = setup();
    const { spy, stop } = feed(mock, store);
    store.stop();
    expect(stop).toHaveBeenCalledTimes(1);
    store.stop(); // idempotent
    expect(stop).toHaveBeenCalledTimes(1);
    store.start();
    expect(spy).toHaveBeenCalledTimes(2);
  });
});

describe('ProcessesStore: view state', () => {
  it('setMode / setQuery filter the rows', () => {
    const { mock, store } = setup();
    feed(mock, store);
    const all = store.rows().length;
    store.setQuery('rustc');
    expect(store.query()).toBe('rustc');
    expect(store.rows().length).toBeGreaterThan(0);
    expect(store.rows().length).toBeLessThan(all);
    store.setMode('flat');
    expect(store.mode()).toBe('flat');
    expect(store.rows().every((r) => r.kind === 'process' && r.name === 'rustc')).toBe(true);
  });
  it('sortBy flips the direction on the same column and picks a sensible default for a new one', () => {
    const { store } = setup();
    expect(store.sort()).toEqual({ key: 'cpu', dir: 'desc' });
    store.sortBy('cpu');
    expect(store.sort()).toEqual({ key: 'cpu', dir: 'asc' });
    store.sortBy('cpu');
    expect(store.sort().dir).toBe('desc');
    store.sortBy('name');
    expect(store.sort()).toEqual({ key: 'name', dir: 'asc' });
    store.sortBy('user');
    expect(store.sort()).toEqual({ key: 'user', dir: 'asc' });
    store.sortBy('memory');
    expect(store.sort()).toEqual({ key: 'memory', dir: 'desc' });
  });
  it('toggle ignores rows that cannot expand', () => {
    const { store } = setup();
    store.toggle({ key: 'gx', expandable: false, expanded: false } as Row);
    expect(store.expanded()).toEqual([]);
  });
  it('toggle expands and collapses a group in groups mode', () => {
    const { mock, store } = setup();
    feed(mock, store);
    const chrome = store.rows().find((r) => r.kind === 'group' && r.expandable)!;
    expect(chrome.expanded).toBe(false);
    store.toggle(chrome);
    expect(store.expanded()).toEqual([chrome.key]);
    const open = store.rows().find((r) => r.key === chrome.key)!;
    expect(open.expanded).toBe(true);
    expect(store.rows().length).toBeGreaterThan(0);
    store.toggle(open);
    expect(store.expanded()).toEqual([]);
  });
  it('expandAll and collapseAll open and fold the groups, and the tree', () => {
    const { mock, store } = setup();
    feed(mock, store);
    const collapsed = store.rows().length;
    store.expandAll();
    expect(store.rows().length).toBeGreaterThan(collapsed);
    expect(store.rows().filter((r) => r.expandable).every((r) => r.expanded)).toBe(true);
    store.collapseAll();
    expect(store.rows()).toHaveLength(collapsed);
    store.setMode('tree');
    store.expandAll();
    expect(store.rows().filter((r) => r.expandable).every((r) => r.expanded)).toBe(true);
    store.collapseAll();
    expect(store.rows().filter((r) => r.depth > 0)).toEqual([]);
    store.setMode('flat');
    store.expandAll();
    expect(store.expanded()).toEqual([]);
  });
  it('toggle in tree mode remembers an explicit collapse of a root', () => {
    const { mock, store } = setup();
    feed(mock, store);
    store.setMode('tree');
    const root = store.rows()[0]!;
    expect(root.expanded).toBe(true); // roots start open
    store.toggle(root);
    expect(store.expanded()).toEqual([`-${root.key}`]);
    expect(store.rows()[0]!.expanded).toBe(false);
    store.toggle(store.rows()[0]!);
    expect(store.expanded()).toEqual([root.key]);
    expect(store.rows()[0]!.expanded).toBe(true);
  });
});

describe('ProcessesStore: selection', () => {
  it('select loads details', async () => {
    const { mock, store } = setup();
    feed(mock, store);
    const p = mock.liveUpdate('processes').processes![0]!;
    const promise = store.select({ pid: p.pid, started: readValue(p.started) });
    expect(store.detailsLoading()).toBe(true);
    expect(store.details()).toBeNull();
    await promise;
    expect(store.details()?.process.pid).toBe(p.pid);
    expect(store.detailsLoading()).toBe(false);
  });
  it('select(null) clears everything', async () => {
    const { mock, store } = setup();
    const p = mock.liveUpdate('processes').processes![0]!;
    await store.select({ pid: p.pid, started: readValue(p.started) });
    await store.select(null);
    expect(store.selected()).toBeNull();
    expect(store.details()).toBeNull();
    expect(store.detailsLoading()).toBe(false);
  });
  it('select failure sets detailsError', async () => {
    const { store } = setup();
    await store.select({ pid: 999999, started: 1 });
    expect(store.detailsError()).toEqual({ code: 'notFound', message: 'The process has exited.' });
    expect(store.detailsLoading()).toBe(false);
  });
  it('ignores a stale answer after another selection', async () => {
    const { mock, store } = setup();
    const [a, b] = mock.liveUpdate('processes').processes!;
    const real = mock.processDetails.bind(mock);
    let release!: (d: ProcessDetails) => void;
    vi.spyOn(mock, 'processDetails').mockImplementationOnce(() => new Promise((r) => (release = r))).mockImplementation(real);
    const first = store.select({ pid: a!.pid, started: readValue(a!.started) });
    await store.select({ pid: b!.pid, started: readValue(b!.started) });
    release(await real({ pid: a!.pid, started: readValue(a!.started) }));
    await first;
    expect(store.details()?.process.pid).toBe(b!.pid);
  });
  it('ignores a stale failure', async () => {
    const { mock, store } = setup();
    const [a, b] = mock.liveUpdate('processes').processes!;
    let fail!: (e: unknown) => void;
    vi.spyOn(mock, 'processDetails').mockImplementationOnce(() => new Promise((_r, rej) => (fail = rej)));
    const first = store.select({ pid: a!.pid, started: readValue(a!.started) });
    await store.select({ pid: b!.pid, started: readValue(b!.started) });
    fail({ code: 'internal', message: 'late' });
    await first;
    expect(store.detailsError()).toBeNull();
    expect(store.details()?.process.pid).toBe(b!.pid);
  });
});

describe('ProcessesStore: requests', () => {

  it('a failed request sets actionError and dismissError clears it', async () => {
    const { mock, store } = setup();
    vi.spyOn(mock, 'openLocation').mockRejectedValueOnce({ code: 'notFound', message: 'gone' });
    await store.openLocation({ pid: 1, started: 1 });
    expect(store.actionError()?.code).toBe('notFound');
    store.dismissError();
    expect(store.actionError()).toBeNull();
  });
  it('lockers resolves a list and rejects with a CommandError', async () => {
    const { store } = setup();
    expect((await store.lockers('/Users/alex/dev/dyno/target')).lockers.length).toBeGreaterThan(0);
    await expect(store.lockers('  ')).rejects.toEqual({ code: 'badRequest', message: 'Enter a file or folder path.' });
  });
  it('lockers normalises odd rejections', async () => {
    const { mock, store } = setup();
    vi.spyOn(mock, 'lockers').mockRejectedValue(new Error('disk gone'));
    await expect(store.lockers('/x')).rejects.toEqual({ code: 'internal', message: 'disk gone' });
  });
  it('openLocation succeeds quietly or sets actionError', async () => {
    const { mock, store } = setup();
    const p = mock.liveUpdate('processes').processes!;
    const withExe = p.find((x) => x.exe)!;
    await store.openLocation({ pid: withExe.pid, started: readValue(withExe.started) });
    expect(store.actionError()).toBeNull();
    const kernel = p.find((x) => !x.exe)!;
    await store.openLocation({ pid: kernel.pid, started: readValue(kernel.started) });
    expect(store.actionError()?.code).toBe('unsupported');
  });
});
