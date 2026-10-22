import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../core/backend';
import { MockBackend } from '../core/mock-backend';
import { Notifier } from '../shared/notifier';
import { NETWORK_POLL_MS, NetworkStore } from './network.store';

const NOW = 1_700_000_000_000;

function setup(fail: string[] = []) {
  const mock = new MockBackend({ latencyMs: 0, now: () => NOW, fail: new Set(fail) });
  TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: mock }] });
  return { mock, store: TestBed.inject(NetworkStore), notifier: TestBed.inject(Notifier) };
}

describe('NetworkStore', () => {
  afterEach(() => vi.useRealTimers());

  it('hides loopback by default and shows it when asked', async () => {
    const { store } = setup();
    await store.load();
    expect(store.snapshot()!.listeners.every((l) => l.exposure !== 'loopback')).toBe(true);
    await store.setQuery({ showLoopback: true });
    expect(store.snapshot()!.listeners.some((l) => l.exposure === 'loopback')).toBe(true);
  });

  it('filters by exposure, protocol, state and search', async () => {
    const { store } = setup();
    await store.setQuery({ exposedOnly: true });
    expect(store.snapshot()!.listeners.every((l) => l.exposure === 'allInterfaces')).toBe(true);
    expect(store.snapshot()!.connections).toEqual([]);
    await store.setQuery({ exposedOnly: false, proto: 'udp' });
    expect(store.snapshot()!.listeners.map((l) => l.port)).toEqual([5353]);
    await store.setQuery({ proto: null, state: 'timeWait' });
    expect(store.snapshot()!.connections.map((c) => c.stateLabel)).toEqual(['TIME_WAIT']);
    await store.setQuery({ state: null, search: 'chrome' });
    expect(store.snapshot()!.connections.every((c) => c.owner.kind === 'process' && c.owner.name === 'Google Chrome')).toBe(true);
  });

  it('polls while open and stops when closed', async () => {
    vi.useFakeTimers();
    const { mock, store } = setup();
    const spy = vi.spyOn(mock, 'network');
    await store.start();
    await vi.advanceTimersByTimeAsync(NETWORK_POLL_MS * 2 + 10);
    expect(spy.mock.calls.length).toBeGreaterThanOrEqual(3);
    store.stop();
    const n = spy.mock.calls.length;
    await vi.advanceTimersByTimeAsync(NETWORK_POLL_MS * 3);
    expect(spy.mock.calls.length).toBe(n);
  });

  it('a failed read shows the error and a lookup of a private address is explained, not opened', async () => {
    const failing = setup(['network']);
    await failing.store.load();
    expect(failing.store.error()).not.toBeNull();
    TestBed.resetTestingModule();
    const { mock, store, notifier } = setup();
    await store.lookup('192.168.1.5');
    expect(mock.opened).toEqual([]);
    expect(notifier.toasts().at(-1)?.text).toContain('nothing to look up');
    await store.lookup('8.8.8.8');
    expect(mock.opened).toEqual(['https://ipinfo.io/8.8.8.8']);
  });
});
