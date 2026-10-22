import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../core/backend';
import { MockBackend } from '../core/mock-backend';
import { Notifier } from '../shared/notifier';
import { CleanupStore } from './cleanup.store';

const NOW = 1_700_000_000_000;

function setup(fail: string[] = []) {
  const mock = new MockBackend({ latencyMs: 0, now: () => NOW, fail: new Set(fail) });
  TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: mock }] });
  return { mock, store: TestBed.inject(CleanupStore), notifier: TestBed.inject(Notifier) };
}

describe('CleanupStore', () => {
  it('loads categories and picks the safe ones to scan', async () => {
    const { store } = setup();
    await store.loadCategories();
    expect(store.categories()!.length).toBeGreaterThan(5);
    const picked = store.picked();
    expect(picked.length).toBeGreaterThan(0);
    expect(picked.every((id) => store.categories()!.find((c) => c.id === id)!.risk === 'safe')).toBe(true);
    store.togglePick(picked[0]!);
    expect(store.picked()).not.toContain(picked[0]);
    store.togglePick(picked[0]!);
    expect(store.picked()).toContain(picked[0]);
    await store.loadCategories();
    expect(store.picked()).toContain(picked[0]);
  });

  it('keeps the engine message when the categories fail', async () => {
    const { store } = setup(['cleanupCategories']);
    await store.loadCategories();
    expect(store.categories()).toBeNull();
    expect(store.categoriesError()?.message).toBeTruthy();
  });

  it('a scan only reads: it pre-selects what the engine did and nothing runs', async () => {
    const { store, mock } = setup();
    await store.loadCategories();
    const run = vi.spyOn(mock, 'runCleanup');
    await store.runScan();
    expect(store.scan()).not.toBeNull();
    expect(store.selection().length).toBeGreaterThan(0);
    expect(run).not.toHaveBeenCalled();
    expect(store.scanning()).toBe(false);
  });

  it('does not scan with nothing picked', async () => {
    const { store, mock } = setup();
    const scan = vi.spyOn(mock, 'scanCleanup');
    await store.runScan();
    expect(scan).not.toHaveBeenCalled();
  });

  it('shows a failed scan inline and clears it on retry', async () => {
    const { store, mock } = setup();
    await store.loadCategories();
    vi.spyOn(mock, 'scanCleanup').mockRejectedValueOnce({ code: 'timeout', message: 'Too slow' });
    await store.runScan();
    expect(store.scanError()?.message).toBe('Too slow');
    expect(store.scan()).toBeNull();
    await store.runScan();
    expect(store.scanError()).toBeNull();
    expect(store.scan()).not.toBeNull();
  });

  it('toggles items and whole categories', async () => {
    const { store } = setup();
    await store.loadCategories();
    await store.runScan();
    const first = store.scan()!.results.find((r) => r.items.length)!;
    store.setCategory(first.category, false);
    expect(first.items.some((i) => store.selection().includes(i.id))).toBe(false);
    store.toggleItem(first.items[0]!.id);
    expect(store.selection()).toEqual(expect.arrayContaining([first.items[0]!.id]));
    store.setCategory(first.category, true);
    expect(first.items.every((i) => store.selection().includes(i.id))).toBe(true);
    store.setCategory('unknown', true);
    store.toggleItem(first.items[0]!.id);
    expect(store.selection()).not.toContain(first.items[0]!.id);
    store.clearSelection();
    expect(store.selection()).toEqual([]);
  });

  it('runs only the selection, consumes the scan, keeps the result for undo and lists the quarantine', async () => {
    const { store, mock, notifier } = setup();
    await store.loadCategories();
    await store.runScan();
    const run = vi.spyOn(mock, 'runCleanup');
    const ids = [...store.selection()];
    await store.run('quarantine');
    expect(run).toHaveBeenCalledWith(expect.any(String), ids, 'quarantine');
    expect(store.scan()).toBeNull();
    expect(store.selection()).toEqual([]);
    expect(store.result()!.outcome.quarantineId).toMatch(/^q-/);
    expect(store.quarantines()!.some((q) => q.id === store.result()!.outcome.quarantineId)).toBe(true);
    expect(notifier.toasts().at(-1)!.text).toMatch(/^Moved \d+ items?/);
    store.dismissResult();
    expect(store.result()).toBeNull();
  });

  it('does nothing without a scan or a selection', async () => {
    const { store, mock } = setup();
    const run = vi.spyOn(mock, 'runCleanup');
    await store.run('quarantine');
    await store.loadCategories();
    await store.runScan();
    store.clearSelection();
    await store.run('recycleBin');
    expect(run).not.toHaveBeenCalled();
  });

  it('tells the person when a run is refused and keeps the scan', async () => {
    const { store, mock, notifier } = setup();
    await store.loadCategories();
    await store.runScan();
    vi.spyOn(mock, 'runCleanup').mockRejectedValueOnce({ code: 'badRequest', message: 'This scan is more than an hour old; scan again before cleaning.' });
    await store.run('quarantine');
    expect(notifier.toasts().at(-1)!.text).toContain('more than an hour old');
    expect(store.scan()).not.toBeNull();
    expect(store.running()).toBe(false);
  });

  it('undoes a run by restoring its quarantine', async () => {
    const { store, notifier } = setup();
    await store.loadCategories();
    await store.runScan();
    await store.run('quarantine');
    const q = store.result()!.outcome.quarantineId!;
    await store.restore(q);
    expect(store.result()).toBeNull();
    expect(store.quarantines()!.some((e) => e.id === q)).toBe(false);
    expect(notifier.toasts().at(-1)!.text).toBe('Put back where it was');
    expect(store.restoring()).toBeNull();
  });

  it('shows a failed restore and keeps the quarantine listed', async () => {
    const { store, notifier } = setup();
    await store.loadQuarantines();
    await store.restore('nope');
    expect(notifier.toasts().at(-1)!.tone).toBe('bad');
    expect(store.quarantines()!.length).toBeGreaterThan(0);
  });

  it('keeps the engine message when the quarantine list fails', async () => {
    const { store } = setup(['quarantine']);
    await store.loadQuarantines();
    expect(store.quarantineError()?.message).toBeTruthy();
  });
});
