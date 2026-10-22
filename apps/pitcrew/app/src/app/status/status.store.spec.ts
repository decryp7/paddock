import { TestBed } from '@angular/core/testing';
import type { PitcrewBackend } from '../core/backend';
import { BACKEND } from '../core/backend';
import { DEMO_RUNNERS, MockBackend } from '../core/mock-backend';
import { StatusStore } from './status.store';

function setup(backend: PitcrewBackend) {
  TestBed.configureTestingModule({ providers: [StatusStore, { provide: BACKEND, useValue: backend }] });
  return TestBed.inject(StatusStore);
}

describe('StatusStore', () => {
  it('loads the runners', async () => {
    const store = setup(new MockBackend());
    expect(store.loaded()).toBe(false);
    const done = store.load();
    expect(store.loading()).toBe(true);
    await done;
    expect(store.loading()).toBe(false);
    expect(store.loaded()).toBe(true);
    expect(store.runners().map((r) => r.name)).toEqual(DEMO_RUNNERS.map((r) => r.name));
    expect(store.error()).toBeNull();
  });

  it('keeps the last good list and shows a typed error when a request fails', async () => {
    let fail = false;
    const backend: PitcrewBackend = {
      status: () => (fail ? Promise.reject({ code: 'notImplemented', message: 'Runner status is not implemented yet' }) : Promise.resolve(DEMO_RUNNERS)),
      appUpdateStatus: () => Promise.reject(new Error('unused')),
    };
    const store = setup(backend);
    await store.load();
    fail = true;
    await store.load();
    expect(store.error()).toEqual({ code: 'notImplemented', message: 'Runner status is not implemented yet' });
    expect(store.runners().length).toBe(DEMO_RUNNERS.length);
    fail = false;
    await store.load();
    expect(store.error()).toBeNull();
  });
});
