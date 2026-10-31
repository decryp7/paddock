import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../../core/backend';
import { MockBackend } from '../../core/mock-backend';
import { DEFAULT_KINDS, LOADING_SHAPE } from '../../core/mock/mock-popover';
import { TrayWidgetsStore } from './tray-widgets.store';

const flush = async (n = 6) => {
  for (let i = 0; i < n; i++) await new Promise((r) => setTimeout(r, 0));
};

function setup(backend: MockBackend = new MockBackend()) {
  TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: backend }] });
  return { backend, store: TestBed.inject(TrayWidgetsStore) };
}

describe('the tray widgets store', () => {
  it('starts empty, loads the layout, values, shape and header together', async () => {
    const { store } = setup();
    expect([store.view(), store.values(), store.header(), store.shape()]).toEqual([null, null, null, []]);
    store.start();
    await flush();
    expect(store.view()!.layout.placements.map((p) => p.widget)).toEqual(DEFAULT_KINDS);
    expect(store.values()!.placements.length).toBe(4);
    expect(store.header()!.status).toEqual({ kind: 'serving', port: 11435 });
    expect(store.shape()).toEqual(LOADING_SHAPE);
    expect(store.shape().find((s) => s.kind === 'loadedModels')!.rows).toBe(2);
    expect(store.error()).toBeNull();
    expect(store.loading()).toBe(false);
  });

  it('starts once: a second start neither loads nor listens again', async () => {
    const { store, backend } = setup();
    const view = vi.spyOn(backend, 'popoverView');
    const listen = vi.spyOn(backend, 'onPopover');
    store.start();
    store.start();
    await flush();
    expect(view).toHaveBeenCalledTimes(1);
    expect(listen).toHaveBeenCalledTimes(1);
  });

  it('keeps the error of a failed read and clears it with the next success', async () => {
    const { store, backend } = setup(new MockBackend({ failing: ['popoverValues'] }));
    store.start();
    await flush();
    expect(store.error()!.code).toBe('serverUnavailable');
    expect(store.view()).toBeNull();
    (backend as unknown as { failing: Set<string> }).failing.clear();
    await store.load();
    expect(store.error()).toBeNull();
    expect(store.view()).not.toBeNull();
  });

  it('applies an edit: the engine\'s answer replaces the layout and the values follow', async () => {
    const { store } = setup();
    store.start();
    await flush();
    const memory = store.view()!.layout.placements.find((p) => p.widget === 'memory')!;
    expect(await store.edit({ remove: { id: memory.id } })).toBe(true);
    expect(store.view()!.layout.placements.map((p) => p.widget)).toEqual(['server', 'activity', 'loadedModels']);
    expect(store.view()!.canUndo).toBe(true);
    expect(store.values()!.placements.length).toBe(3);
    expect(await store.edit('undo')).toBe(true);
    expect(store.view()!.layout.placements.length).toBe(4);
  });

  it('keeps the layout and says why when the engine refuses an edit', async () => {
    const { store } = setup();
    store.start();
    await flush();
    expect(await store.edit({ add: { widget: 'memory', span: null, at: null } })).toBe(false);
    expect(store.error()!.message).toBe('Memory is already shown.');
    expect(store.view()!.layout.placements.length).toBe(4);
  });

  it('follows what the app pushes: a changed layout, values and header replace the old, and clear an error', async () => {
    const { store, backend } = setup();
    store.start();
    await flush();
    await store.edit({ add: { widget: 'memory', span: null, at: null } });
    expect(store.error()).not.toBeNull();
    await backend.setServing(true);
    await flush();
    expect(store.header()!.status).toEqual({ kind: 'paused' });
    expect(store.header()!.serving).toBe('resume');
    expect(store.error()).toBeNull();
  });

  it('a push that arrives while the first read runs is not overwritten by the older answer', async () => {
    const backend = new MockBackend();
    const { store } = setup(backend);
    let release: () => void = () => undefined;
    const gate = new Promise<void>((r) => (release = r));
    const read = backend.popoverValues.bind(backend);
    vi.spyOn(backend, 'popoverValues').mockImplementation(async () => {
      const older = await read();
      await gate;
      return older;
    });
    store.start();
    await flush(2);
    await backend.setServing(true); // pushes the paused header and values while the read waits
    await flush(2);
    expect(store.header()!.status).toEqual({ kind: 'paused' });
    release();
    await flush();
    expect(store.header()!.status, 'the older read did not undo the push').toEqual({ kind: 'paused' });
    expect(store.shape().length).toBe(7);
    expect(store.view()).not.toBeNull();
  });
});
