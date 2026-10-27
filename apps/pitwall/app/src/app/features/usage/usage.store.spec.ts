import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../../core/backend';
import { MockBackend } from '../../core/mock-backend';
import { UsageStore } from './usage.store';

function setup(backend = new MockBackend()) {
  TestBed.configureTestingModule({ providers: [UsageStore, { provide: BACKEND, useValue: backend }] });
  return { store: TestBed.inject(UsageStore), backend };
}

describe('UsageStore', () => {
  it('shows all apps together without a detail', async () => {
    const { store } = setup();
    const done = store.show(null, '7d');
    expect(store.loading()).toBe(true);
    await done;
    expect(store.loading()).toBe(false);
    expect(store.series()!.range).toBe('7d');
    expect(store.series()!.starts.length).toBe(168);
    expect(store.series()!.apps.length).toBeGreaterThan(1);
    expect(store.detail()).toBeNull();
  });

  it('shows one app with its detail and switches range', async () => {
    const { store } = setup();
    await store.show('spotter');
    expect(store.series()!.apps.map((a) => a.appId)).toEqual(['spotter']);
    expect(store.detail()!.app.id).toBe('spotter');
    expect(store.detail()!.limits.length).toBeGreaterThan(0);
    await store.select('30d');
    expect(store.range()).toBe('30d');
    expect(store.series()!.starts.length).toBe(30);
  });

  it('drops the previous app while another one loads', async () => {
    const { store } = setup();
    await store.show('spotter');
    const next = store.show('pitcrew');
    expect(store.series()).toBeNull();
    expect(store.detail()).toBeNull();
    await next;
    expect(store.detail()!.app.id).toBe('pitcrew');
  });

  it('keeps the last good data and a typed error when loading fails, then recovers on reload', async () => {
    const { store, backend } = setup();
    await store.show(null);
    const before = store.series();
    vi.spyOn(backend, 'usageSeries').mockRejectedValueOnce({ code: 'serverUnavailable', message: 'down' });
    await store.reload();
    expect(store.error()).toEqual({ code: 'serverUnavailable', message: 'down' });
    expect(store.series()).toBe(before);
    await store.reload();
    expect(store.error()).toBeNull();
  });

  it('ignores a late answer for a range that is no longer selected', async () => {
    const { store, backend } = setup();
    let release!: (v: Awaited<ReturnType<typeof backend.usageSeries>>) => void;
    const slow = new Promise<Awaited<ReturnType<typeof backend.usageSeries>>>((r) => (release = r));
    const real = backend.usageSeries.bind(backend);
    vi.spyOn(backend, 'usageSeries').mockImplementationOnce(() => slow);
    const first = store.show(null, '24h');
    await store.select('30d');
    release(await real('24h', null));
    await first;
    expect(store.series()!.range).toBe('30d');
    expect(store.loading()).toBe(false);
  });

  it('ignores a late failure for an app that is no longer selected', async () => {
    const { store, backend } = setup();
    let fail!: (e: unknown) => void;
    vi.spyOn(backend, 'usageSeries').mockImplementationOnce(() => new Promise((_, rej) => (fail = rej)));
    const first = store.show('debrief');
    await store.show('dyno');
    fail({ code: 'internal', message: 'late' });
    await first;
    expect(store.error()).toBeNull();
    expect(store.detail()!.app.id).toBe('dyno');
  });
});
