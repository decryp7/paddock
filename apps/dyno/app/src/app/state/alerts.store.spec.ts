import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../core/backend';
import { MockBackend } from '../core/mock-backend';
import type { Alert } from '../core/api.types';
import { AlertsStore, mergeAlert } from './alerts.store';

const NOW = 1_700_000_000_000;
const alert = (id: string, at: number, title = id): Alert => ({ id, at, kind: 'lowDisk', severity: 'warning', title, detail: '' });

function setup() {
  const mock = new MockBackend({ latencyMs: 0, now: () => NOW });
  TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: mock }] });
  return { mock, store: TestBed.inject(AlertsStore) };
}

describe('mergeAlert', () => {
  it('puts the new alert in newest-first order', () => {
    expect(mergeAlert([alert('a', 3), alert('b', 1)], alert('c', 2)).map((a) => a.id)).toEqual(['a', 'c', 'b']);
  });
  it('replaces an alert with the same id', () => {
    const out = mergeAlert([alert('a', 3), alert('b', 1)], alert('b', 5, 'again'));
    expect(out.map((a) => a.title)).toEqual(['again', 'a']);
  });
  it('caps the list', () => {
    expect(mergeAlert([alert('a', 3), alert('b', 2)], alert('c', 1), 2).map((a) => a.id)).toEqual(['a', 'b']);
  });
});

describe('AlertsStore', () => {
  it('loads alerts', async () => {
    const { store } = setup();
    await store.load(3);
    expect(store.loaded()).toBe(true);
    expect(store.alerts().length).toBeGreaterThan(0);
    expect(store.alerts().length).toBeLessThanOrEqual(3);
  });
  it('records a load failure', async () => {
    const { store, mock } = setup();
    vi.spyOn(mock, 'alerts').mockRejectedValue({ code: 'timeout', message: 'slow' });
    await store.load();
    expect(store.error()).toEqual({ code: 'timeout', message: 'slow' });
    expect(store.loading()).toBe(false);
  });
  it('receives pushed alerts', () => {
    const { store, mock } = setup();
    mock.pushAlert(alert('new', NOW));
    expect(store.alerts()[0]?.id).toBe('new');
  });
  it('stops listening on destroy', () => {
    const mock = new MockBackend({ latencyMs: 0 });
    const stop = vi.fn();
    vi.spyOn(mock, 'onAlert').mockReturnValue(stop);
    TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: mock }] });
    TestBed.inject(AlertsStore);
    TestBed.resetTestingModule();
    expect(stop).toHaveBeenCalled();
  });
});
