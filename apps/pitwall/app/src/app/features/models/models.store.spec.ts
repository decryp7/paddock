import { TestBed } from '@angular/core/testing';
import type { PitwallBackend } from '../../core/backend';
import { BACKEND } from '../../core/backend';
import { MockBackend, type MockOptions } from '../../core/mock-backend';
import { Toasts } from '../../core/toasts';
import { ModelsStore } from './models.store';

function setup(backend: PitwallBackend = new MockBackend()) {
  TestBed.configureTestingModule({ providers: [ModelsStore, { provide: BACKEND, useValue: backend }] });
  return { store: TestBed.inject(ModelsStore), toasts: TestBed.inject(Toasts), backend };
}

const failing = (...methods: (keyof PitwallBackend)[]) => new MockBackend({ failing: methods } as MockOptions);

describe('ModelsStore', () => {
  it('loads the library and disk usage, with the unused models', async () => {
    const { store } = setup();
    const done = store.load();
    expect(store.loading()).toBe(true);
    await done;
    expect(store.loaded()).toBe(true);
    expect(store.library().length).toBe(8);
    expect(store.disk()?.unusedAfterDays).toBe(30);
    expect(store.unusedModels().map((m) => m.id)).toEqual(['qwen3.5-2b-q8_0']);
    expect(store.unusedBytes()).toBeGreaterThan(0);
  });

  it('shows a load error and keeps nothing stale', async () => {
    const { store } = setup(failing('localModels'));
    await store.load();
    expect(store.error()?.code).toBe('serverUnavailable');
    expect(store.loaded()).toBe(false);
    expect(store.unusedModels()).toEqual([]);
    expect(store.unusedBytes()).toBe(0);
  });

  it('searches and keeps only the latest query', async () => {
    const { store } = setup();
    const first = store.search('qwen');
    const second = store.search('gemma');
    await Promise.all([first, second]);
    expect(store.query()).toBe('gemma');
    expect(store.results().every((r) => r.repo.toLowerCase().includes('gemma'))).toBe(true);
    expect(store.searched()).toBe(true);
  });

  it('reports a search error, and ignores the error of a superseded query', async () => {
    const { store } = setup(failing('searchModels'));
    await store.search('x');
    expect(store.searchError()?.code).toBe('serverUnavailable');
    const a = store.search('a');
    const b = store.search('b');
    await Promise.all([a, b]);
    expect(store.query()).toBe('b');
    expect(store.searching()).toBe(false);
  });

  it('opens and closes a repo; a superseded repo is ignored', async () => {
    const { store } = setup();
    const a = store.openRepo('unsloth/Qwen3.5-9B-GGUF');
    const b = store.openRepo('unsloth/gemma-4-12b-it-GGUF');
    await Promise.all([a, b]);
    expect(store.repo()?.repo).toBe('unsloth/gemma-4-12b-it-GGUF');
    expect(store.repo()?.projectors.length).toBe(1);
    store.closeRepo();
    expect(store.repoName()).toBeNull();
    expect(store.repo()).toBeNull();
  });

  it('reports a repo error', async () => {
    const { store } = setup(failing('repoFiles'));
    await store.openRepo('a/b');
    expect(store.repoError()?.code).toBe('serverUnavailable');
    expect(store.repoLoading()).toBe(false);
    const c = store.openRepo('c/d');
    store.closeRepo();
    await c;
    expect(store.repoError()).toBeNull();
  });

  it('downloads a variant of the open repo (with its projector) and cancels downloads', async () => {
    const mock = new MockBackend();
    const { store, toasts } = setup(mock);
    expect(await store.download({ file: 'x.gguf' } as never, null)).toBe(false);
    await store.openRepo('unsloth/gemma-4-12b-it-GGUF');
    const v = store.repo()!.variants[0]!;
    expect(await store.download(v, 'mmproj-F16.gguf')).toBe(true);
    const list = await mock.downloads();
    expect(list[0]!.name).toBe(v.file);
    expect(toasts.toasts().at(-1)?.message).toContain('Downloading');
    expect(await store.cancelDownload(list[0]!.id)).toBe(true);
    expect((await mock.downloads())[0]!.state).toBe('cancelled');
    expect(await store.cancelDownload('nope')).toBe(false);
    expect(toasts.toasts().at(-1)?.tone).toBe('bad');
  });

  it('loads, unloads, pins and sets the idle time of models', async () => {
    const { store } = setup();
    await store.load();
    await store.loadModel('gemma-4-12b-it-q4_k_m');
    expect(store.library().find((m) => m.id === 'gemma-4-12b-it-q4_k_m')?.loaded).toBe(true);
    await store.unloadModel('gemma-4-12b-it-q4_k_m');
    expect(store.library().find((m) => m.id === 'gemma-4-12b-it-q4_k_m')?.loaded).toBe(false);
    await store.pinModel('qwen3.5-9b-q4_k_m', true);
    expect(store.library().find((m) => m.id === 'qwen3.5-9b-q4_k_m')?.pinned).toBe(true);
    await store.pinModel('qwen3.5-9b-q4_k_m', false);
    await store.setIdle('qwen3.5-9b-q4_k_m', 300);
    expect(store.library().find((m) => m.id === 'qwen3.5-9b-q4_k_m')?.idleSecs).toBe(300);
    await store.setIdle('qwen3.5-9b-q4_k_m', null);
    expect(store.library().find((m) => m.id === 'qwen3.5-9b-q4_k_m')?.idleSecs).toBeNull();
  });

  it('keeps the library when an action fails and says why', async () => {
    const { store, toasts } = setup();
    await store.load();
    await store.setIdle('qwen3.5-9b-q4_k_m', 5);
    expect(toasts.toasts().at(-1)?.message).toContain('at least 30 seconds');
    expect(store.library().find((m) => m.id === 'qwen3.5-9b-q4_k_m')?.idleSecs).toBeNull();
    await store.pinModel('nope', true);
    await store.unloadModel('gemma-4-12b-it-q4_k_m');
    await store.deleteModel('qwen3.5-9b-q4_k_m');
    expect(toasts.toasts().at(-1)?.message).toContain('Unload the model');
    expect(store.library().length).toBe(8);
  });

  it('ignores a second action on the same model while one is running', async () => {
    const { store } = setup();
    await store.load();
    const a = store.loadModel('gemma-4-12b-it-q4_k_m');
    expect(store.isBusy()('model:gemma-4-12b-it-q4_k_m')).toBe(true);
    const b = store.unloadModel('gemma-4-12b-it-q4_k_m');
    await Promise.all([a, b]);
    expect(store.library().find((m) => m.id === 'gemma-4-12b-it-q4_k_m')?.loaded).toBe(true);
  });

  it('deletes a model and cleans up unused ones', async () => {
    const { store, toasts } = setup();
    await store.load();
    await store.deleteModel('bge-reranker-v2-m3-q8_0');
    expect(store.library().some((m) => m.id === 'bge-reranker-v2-m3-q8_0')).toBe(false);
    await store.cleanUnused();
    expect(store.library().some((m) => m.id === 'qwen3.5-2b-q8_0')).toBe(false);
    expect(toasts.toasts().at(-1)?.message).toMatch(/^Freed /);
    expect(store.unusedModels()).toEqual([]);
    const before = toasts.toasts().length;
    await store.cleanUnused();
    expect(toasts.toasts().length).toBe(before);
  });

  it('reloads after a clean-up that failed part-way', async () => {
    const { store, toasts } = setup(failing('deleteModel'));
    await store.load();
    await store.cleanUnused();
    expect(toasts.toasts().at(-1)?.tone).toBe('bad');
    expect(store.library().length).toBe(8);
  });
});

describe('ModelsStore slots', () => {
  it('saves slots, keeps the library in step and says when a loaded model picks them up', async () => {
    const { store, toasts } = setup();
    await store.load();
    expect(await store.setSlots('qwen3.5-9b-q4_k_m', 8)).toBe(true);
    const m = store.library().find((x) => x.id === 'qwen3.5-9b-q4_k_m')!;
    expect([m.slots, m.slotsCustom, m.loadedSlots]).toEqual([8, true, 2]);
    expect(toasts.toasts().at(-1)?.message).toContain('keeps 2 slots until it loads again');
    expect(await store.setSlots('gemma-4-12b-it-q4_k_m', 1)).toBe(true);
    expect(toasts.toasts().at(-1)?.message).toBe('1 slot saved.');
    expect(await store.setSlots('gemma-4-12b-it-q4_k_m', null)).toBe(true);
    expect(store.library().find((x) => x.id === 'gemma-4-12b-it-q4_k_m')).toMatchObject({ slots: 2, slotsCustom: false });
  });

  it('reports a refused change and leaves the library alone', async () => {
    const { store, toasts } = setup(new MockBackend());
    await store.load();
    expect(await store.setSlots('gemma-4-12b-it-q4_k_m', 99)).toBe(false);
    expect(toasts.toasts().at(-1)?.tone).toBe('bad');
    expect(store.library().find((x) => x.id === 'gemma-4-12b-it-q4_k_m')?.slots).toBe(2);
  });
});
