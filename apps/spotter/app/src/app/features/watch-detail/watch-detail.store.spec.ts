import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../../core/backend';
import { makeMock } from '../../../testing/test-helpers';
import { WatchDetailStore } from './watch-detail.store';

function setup() {
  const backend = makeMock();
  TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: backend }, WatchDetailStore] });
  return { backend, store: TestBed.inject(WatchDetailStore) };
}

describe('WatchDetailStore', () => {
  afterEach(() => vi.restoreAllMocks());

  it('loads a watch by id and reloads it', async () => {
    const { backend, store } = setup();
    await store.reload(); // nothing loaded yet
    expect(backend.calls).toEqual([]);
    const p = store.load('w-1');
    expect(store.detail().loading).toBe(true);
    await p;
    expect(store.detail().value?.summary.name).toBe('Acme Installer');
    await store.reload();
    expect(backend.calls).toEqual(['getWatch', 'getWatch']);
  });

  it('shows an error and offers a retry', async () => {
    const { store } = setup();
    await store.load('missing');
    expect(store.detail().error?.code).toBe('notFound');
    expect(store.detail().value).toBeNull();
    await store.load('w-2');
    expect(store.detail().error).toBeNull();
  });

  it('keeps the shown detail while the same watch refreshes, and drops it for another', async () => {
    const { store } = setup();
    await store.load('w-1');
    const p = store.load('w-1');
    expect(store.detail().value).not.toBeNull();
    await p;
    const q = store.load('w-2');
    expect(store.detail().value).toBeNull();
    await q;
  });

  it('drops a slow answer for a watch the user already left', async () => {
    const { backend, store } = setup();
    const real = backend.getWatch.bind(backend);
    let release!: () => void;
    const gate = new Promise<void>((r) => (release = r));
    vi.spyOn(backend, 'getWatch').mockImplementation(async (id) => {
      if (id === 'w-1') await gate;
      return real(id);
    });
    const slow = store.load('w-1');
    await store.load('w-2');
    release();
    await slow;
    expect(store.id()).toBe('w-2');
    expect(store.detail().value?.summary.id).toBe('w-2');
  });
});
