import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../core/backend';
import { MockBackend } from '../core/mock-backend';
import type { History } from '../core/api.types';
import { HistoryStore } from './history.store';

const NOW = 1_700_000_000_000;

function setup() {
  const mock = new MockBackend({ latencyMs: 0, now: () => NOW });
  TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: mock }] });
  return { mock, store: TestBed.inject(HistoryStore) };
}

describe('HistoryStore', () => {
  it('loads the current range', async () => {
    const { store, mock } = setup();
    const spy = vi.spyOn(mock, 'history');
    await store.load();
    expect(spy).toHaveBeenCalledWith('day', null);
    expect(store.history()?.range).toBe('day');
    expect(store.loading()).toBe(false);
  });
  it('records a failure', async () => {
    const { store, mock } = setup();
    vi.spyOn(mock, 'history').mockRejectedValue({ code: 'internal', message: 'no' });
    await store.load();
    expect(store.error()).toEqual({ code: 'internal', message: 'no' });
    expect(store.history()).toBeNull();
  });
  it('ensure loads once', async () => {
    const { store, mock } = setup();
    const spy = vi.spyOn(mock, 'history');
    await store.ensure();
    await store.ensure();
    expect(spy).toHaveBeenCalledTimes(1);
  });
  it('setRange switches ranges and reloads', async () => {
    const { store } = setup();
    await store.load();
    await store.setRange('week');
    expect(store.range()).toBe('week');
    expect(store.history()?.range).toBe('week');
  });
  it('setRange to the loaded range does nothing', async () => {
    const { store, mock } = setup();
    await store.load();
    const spy = vi.spyOn(mock, 'history');
    await store.setRange('day');
    expect(spy).not.toHaveBeenCalled();
  });
  it('setRange to the current range reloads when nothing is loaded yet', async () => {
    const { store, mock } = setup();
    const spy = vi.spyOn(mock, 'history');
    await store.setRange('day');
    expect(spy).toHaveBeenCalledTimes(1);
  });
  it('ignores a slow answer for an old range', async () => {
    const { store, mock } = setup();
    const real = mock.history.bind(mock);
    let release!: (h: History) => void;
    const slow = new Promise<History>((r) => (release = r));
    vi.spyOn(mock, 'history').mockImplementationOnce(() => slow).mockImplementation(real);
    const first = store.load(); // day, slow
    await store.setRange('month'); // newest
    release(await real('hour', null));
    await first;
    expect(store.history()?.range).toBe('month');
    expect(store.loading()).toBe(false);
  });
  it('ignores a late failure for an old request', async () => {
    const { store, mock } = setup();
    const real = mock.history.bind(mock);
    let fail!: (e: unknown) => void;
    const slow = new Promise<History>((_r, rej) => (fail = rej));
    vi.spyOn(mock, 'history').mockImplementationOnce(() => slow).mockImplementation(real);
    const first = store.load();
    await store.setRange('week');
    fail({ code: 'internal', message: 'late' });
    await first;
    expect(store.error()).toBeNull();
    expect(store.history()?.range).toBe('week');
  });
});
