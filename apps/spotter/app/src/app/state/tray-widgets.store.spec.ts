import { TestBed } from '@angular/core/testing';
import { provideShellTesting } from '@decryp7/paddock-ui/shell/testing';
import { BACKEND } from '../core/backend';
import { makeMock } from '../../testing/test-helpers';
import { TrayWidgetsStore } from './tray-widgets.store';

function setup() {
  const backend = makeMock();
  TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: backend }, ...provideShellTesting()] });
  return { backend, store: TestBed.inject(TrayWidgetsStore) };
}

const down = { code: 'unavailable', message: 'The engine is not answering.', problems: [] };

describe('TrayWidgetsStore', () => {
  afterEach(() => vi.restoreAllMocks());

  it('loads the engine layout and its values together', async () => {
    const { store } = setup();
    expect(store.view()).toBeNull();
    await store.load();
    expect(store.view()?.layout.placements.length).toBeGreaterThan(0);
    expect(store.values()?.placements).toHaveLength(store.view()?.layout.placements.length ?? -1);
    expect(store.loading()).toBe(false);
    expect(store.error()).toBeNull();
  });

  it('starts once, and follows what the engine pushes', async () => {
    const { backend, store } = setup();
    store.start();
    store.start();
    await vi.waitFor(() => expect(store.view()).not.toBeNull());
    expect(backend.calls.filter((c) => c === 'popoverView')).toHaveLength(1);
    await backend.editPopover({ remove: { id: 'failures' } });
    await vi.waitFor(() => expect(store.view()?.layout.placements.some((p) => p.id === 'failures')).toBe(false));
    expect(store.values()?.placements.some((p) => p.id === 'failures')).toBe(false);
  });

  it('keeps a failed read as an error, and clears it when the next read works', async () => {
    const { backend, store } = setup();
    const real = backend.popoverView.bind(backend);
    vi.spyOn(backend, 'popoverView').mockRejectedValueOnce(down);
    await store.load();
    expect(store.error()?.message).toBe(down.message);
    expect(store.view()).toBeNull();
    vi.spyOn(backend, 'popoverView').mockImplementation(real);
    await store.load();
    expect(store.error()).toBeNull();
    expect(store.view()).not.toBeNull();
  });

  it('sends an edit to the engine and shows its answer, values included', async () => {
    const { backend, store } = setup();
    await store.load();
    await expect(store.edit({ remove: { id: 'failures' } })).resolves.toBe(true);
    expect(store.view()?.canUndo).toBe(true);
    expect(store.view()?.layout.placements.some((p) => p.id === 'failures')).toBe(false);
    expect(store.values()?.placements.some((p) => p.id === 'failures')).toBe(false);
    expect(backend.calls).toContain('editPopover');
  });

  it('keeps the layout and says why when the engine refuses an edit', async () => {
    const { store } = setup();
    await store.load();
    const before = store.view();
    await expect(store.edit({ add: { widget: 'failures', span: null, at: null } })).resolves.toBe(false);
    expect(store.view()).toBe(before);
    expect(store.error()?.message).toContain('already shown');
  });
});
