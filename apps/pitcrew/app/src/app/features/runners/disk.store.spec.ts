import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../../core/backend';
import { MockBackend } from '../../core/mock-backend';
import { flush, makeMock } from '../../../testing/test-helpers';
import { StatusStore } from '../../state/status.store';
import { DiskStore } from './disk.store';

function setup() {
  const mock = makeMock();
  TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: mock }, DiskStore] });
  return { mock, store: TestBed.inject(DiskStore) };
}

describe('DiskStore', () => {
  let mock: MockBackend;
  afterEach(() => {
    mock.dispose();
    TestBed.resetTestingModule();
  });

  it('loads a runner’s disk', async () => {
    const s = setup();
    mock = s.mock;
    await s.store.load('docker-lab');
    expect(s.store.runner()).toBe('docker-lab');
    expect(s.store.disk().value?.scan.entries.length).toBeGreaterThan(0);
    expect(s.store.report()).toBeNull();
  });

  it('keeps the old value while reloading the same runner and drops it for another', async () => {
    const s = setup();
    mock = s.mock;
    await s.store.load('docker-lab');
    const first = s.store.disk().value;
    const p = s.store.load('docker-lab');
    expect(s.store.disk()).toMatchObject({ loading: true, value: first });
    await p;
    const q = s.store.load('ui-tests');
    expect(s.store.disk()).toMatchObject({ loading: true, value: null });
    await q;
  });

  it('ignores a slow answer for a runner that is no longer shown', async () => {
    const s = setup();
    mock = s.mock;
    let release!: () => void;
    const real = mock.disk.bind(mock);
    vi.spyOn(mock, 'disk').mockImplementationOnce(async (r) => {
      await new Promise<void>((res) => (release = res));
      return real(r);
    });
    const slow = s.store.load('docker-lab');
    await s.store.load('ui-tests');
    release();
    await slow;
    expect(s.store.runner()).toBe('ui-tests');
    expect(s.store.disk().value?.scan.entries[0]?.path).toBeDefined();
    expect(s.store.runner()).toBe('ui-tests');
  });

  it('records errors', async () => {
    const s = setup();
    mock = s.mock;
    await s.store.load('nope');
    expect(s.store.disk().error?.code).toBe('unknownRunner');
  });

  it('cleans now: keeps the report and reloads', async () => {
    const s = setup();
    mock = s.mock;
    await flush();
    await s.store.clean();
    expect(s.store.report()).toBeNull();
    await s.store.load('docker-lab');
    const status = TestBed.inject(StatusStore);
    await flush();
    expect(status.runners().length).toBeGreaterThan(0);
    await s.store.clean();
    expect(s.store.report()?.trigger).toBe('manual');
    expect(s.store.disk().loading).toBe(false);
  });

  it('keeps no report when the cleanup failed', async () => {
    const s = setup();
    mock = s.mock;
    await s.store.load('docker-lab');
    vi.spyOn(mock, 'cleanDisk').mockRejectedValueOnce({ code: 'busy', message: 'x' });
    await s.store.clean();
    expect(s.store.report()).toBeNull();
  });
});
