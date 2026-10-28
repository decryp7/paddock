import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../../core/backend';
import { MockBackend } from '../../core/mock-backend';
import { makeMock } from '../../../testing/test-helpers';
import { CacheStore } from './cache.store';

describe('CacheStore', () => {
  let mock: MockBackend;
  let store: InstanceType<typeof CacheStore>;
  beforeEach(() => {
    mock = makeMock();
    TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: mock }, CacheStore] });
    store = TestBed.inject(CacheStore);
  });
  afterEach(() => {
    mock.dispose();
    TestBed.resetTestingModule();
  });

  it('loads a runner’s cache and retries after an error', async () => {
    vi.spyOn(mock, 'cacheReport').mockRejectedValueOnce({ code: 'unknownRunner', message: 'no such runner' });
    await store.load('docker-lab');
    expect(store.report().error?.code).toBe('unknownRunner');
    await store.retry();
    expect(store.report().value?.entries).toBeGreaterThan(0);
    expect(store.report().error).toBeNull();
  });

  it('retry without a runner does nothing', async () => {
    const spy = vi.spyOn(mock, 'cacheReport');
    await store.retry();
    expect(spy).not.toHaveBeenCalled();
  });

  it('maps the form to limits: GB to MB, days floored, either one is enough', () => {
    expect(store.limits()).toBeNull();
    store.setMaxGb(2.5);
    expect(store.limits()).toEqual({ maxMb: 2500, maxAgeDays: 0 });
    store.setMaxGb(null);
    store.setMaxAgeDays(14.7);
    expect(store.limits()).toEqual({ maxMb: 0, maxAgeDays: 14 });
    store.setMaxGb(0);
    store.setMaxAgeDays(-1);
    expect(store.limits()).toBeNull();
  });

  it('refuses to preview without limits and says what to set', async () => {
    await store.load('docker-lab');
    const spy = vi.spyOn(mock, 'previewCacheClean');
    expect(store.canPreview()).toBe(false);
    await store.runPreview();
    expect(spy).not.toHaveBeenCalled();
    expect(store.actionError()?.code).toBe('badRequest');
    store.setMaxGb(1);
    expect(store.actionError()).toBeNull();
  });

  it('does nothing on preview or clean without a runner', async () => {
    store.setMaxGb(1);
    await store.runPreview();
    expect(store.preview()).toBeNull();
    expect(await store.clean()).toBe(false);
  });

  it('previews, invalidates the preview when the limits change, then cleans the same limits', async () => {
    await store.load('docker-lab');
    store.setMaxGb(1);
    store.setMaxAgeDays(30);
    await store.runPreview();
    expect(store.currentPreview()?.entries.length).toBeGreaterThan(0);
    expect(store.currentPreview()?.entries.map(([, r]) => r)).toContain('overLimit');
    store.setMaxGb(2);
    expect(store.currentPreview()).toBeNull();
    expect(store.canClean()).toBe(false);
    const clean = vi.spyOn(mock, 'cleanCache');
    expect(await store.clean()).toBe(false);
    expect(clean).not.toHaveBeenCalled();
    await store.runPreview();
    const before = store.report().value!.bytes;
    expect(await store.clean()).toBe(true);
    expect(clean).toHaveBeenCalledWith('docker-lab', { maxMb: 2000, maxAgeDays: 30 });
    expect(store.result()?.failures).toEqual([]);
    expect(store.preview()).toBeNull();
    expect(store.report().value!.bytes).toBeLessThan(before);
  });

  it('shows a busy error from the clean', async () => {
    await store.load('docker-lab');
    store.setMaxAgeDays(1);
    await store.runPreview();
    vi.spyOn(mock, 'cleanCache').mockRejectedValueOnce({ code: 'busy', message: 'a job is running' });
    expect(await store.clean()).toBe(false);
    expect(store.actionError()?.code).toBe('busy');
    expect(store.cleaning()).toBe(false);
  });

  it('records a failed preview', async () => {
    await store.load('docker-lab');
    store.setMaxGb(1);
    vi.spyOn(mock, 'previewCacheClean').mockRejectedValueOnce({ code: 'internal', message: 'boom' });
    await store.runPreview();
    expect(store.actionError()?.message).toBe('boom');
    expect(store.previewing()).toBe(false);
  });

  it('drops the preview and result when another runner is shown, and keeps them on a reload', async () => {
    await store.load('docker-lab');
    store.setMaxAgeDays(1);
    await store.runPreview();
    await store.load('docker-lab');
    expect(store.preview()).not.toBeNull();
    await store.load('ui-tests');
    expect(store.preview()).toBeNull();
    expect(store.runner()).toBe('ui-tests');
  });

  it('ignores a slow answer for a runner that is no longer shown', async () => {
    let release!: () => void;
    const real = mock.cacheReport.bind(mock);
    vi.spyOn(mock, 'cacheReport').mockImplementationOnce(async (r) => {
      await new Promise<void>((res) => (release = res));
      return real(r);
    });
    const slow = store.load('docker-lab');
    await store.load('ui-tests');
    release();
    await slow;
    expect(store.runner()).toBe('ui-tests');
    expect(store.report().value?.dir).toContain('ui-tests');
  });
});
