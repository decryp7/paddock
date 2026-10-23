import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../core/backend';
import { MockBackend } from '../core/mock-backend';
import { Notifier } from '../shared/notifier';
import { WatchedStore } from './watched.store';

function setup(fail: string[] = []) {
  const mock = new MockBackend({ latencyMs: 0, now: () => 1_700_000_000_000, fail: new Set(fail) });
  TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: mock }] });
  return { mock, store: TestBed.inject(WatchedStore), notifier: TestBed.inject(Notifier) };
}

describe('WatchedStore', () => {
  it('loads the folders', async () => {
    const { store } = setup();
    await store.load();
    expect(store.overview()!.folders).toHaveLength(3);
    expect(store.error()).toBeNull();
  });

  it('reports a failed load for Retry', async () => {
    const { store } = setup(['watchedFolders']);
    await store.load();
    expect(store.overview()).toBeNull();
    expect(store.error()?.code).toBe('internal');
  });

  it('adds a folder switched off', async () => {
    const { store } = setup();
    await store.load();
    expect(await store.add({ template: 'installers', keepForSecs: 600, recursive: false })).toBe(true);
    const added = store.overview()!.folders.at(-1)!.folder;
    expect(added).toMatchObject({ enabled: false, previewed: false, keepForSecs: 600 });
    expect(store.busy()).toBeNull();
  });

  it('a refused add shows the reason and changes nothing', async () => {
    const { store, notifier } = setup();
    await store.load();
    expect(await store.add({ template: null, keepForSecs: 5, recursive: false })).toBe(false);
    expect(notifier.toasts().at(-1)).toMatchObject({ tone: 'bad' });
    expect(store.overview()!.folders).toHaveLength(3);
  });

  it('turning a folder on needs the preview, then works', async () => {
    const { store, notifier } = setup();
    await store.load();
    const id = store.overview()!.folders[2]!.folder.id;
    expect(await store.change(id, { enabled: true })).toBe(false);
    expect(notifier.toasts().at(-1)?.text).toContain('preview first');
    await store.loadPreview(id);
    expect(store.preview()?.id).toBe(id);
    expect(store.overview()!.folders[2]!.folder.previewed).toBe(true);
    expect(await store.change(id, { enabled: true }, 'Watching')).toBe(true);
    expect(store.overview()!.folders[2]!.folder.enabled).toBe(true);
    expect(notifier.toasts().at(-1)?.text).toBe('Watching');
  });

  it('a preview of a vanished folder reports its error; closing clears it', async () => {
    const { store } = setup();
    await store.load();
    await store.loadPreview('w-nope');
    expect(store.preview()).toBeNull();
    expect(store.previewError()?.message).toContain('no longer watches');
    store.closePreview();
    expect(store.previewError()).toBeNull();
  });

  it('sweeps after a preview and removes a folder', async () => {
    const { store, mock } = setup();
    await store.load();
    const id = store.overview()!.folders[0]!.folder.id;
    expect(await store.sweep(id)).toBe(true);
    expect(store.overview()!.folders[0]!.folder.lastSweep?.moved).toBeGreaterThan(0);
    expect((await mock.activity(5))[0]?.title).toContain('old files');
    expect(await store.remove(id)).toBe(true);
    expect(store.overview()!.folders).toHaveLength(2);
    expect(await store.remove(id)).toBe(false);
  });

  it('ignores a second request while one is running', async () => {
    const { store } = setup();
    await store.load();
    const id = store.overview()!.folders[0]!.folder.id;
    const first = store.change(id, { keepForSecs: 7200 });
    expect(await store.change(id, { keepForSecs: 3600 })).toBe(false);
    await first;
    expect(store.overview()!.folders[0]!.folder.keepForSecs).toBe(7200);
  });
});
