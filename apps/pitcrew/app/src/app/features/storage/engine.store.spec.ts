import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../../core/backend';
import { MockBackend } from '../../core/mock-backend';
import { makeMock } from '../../../testing/test-helpers';
import { EngineStore } from './engine.store';

describe('EngineStore', () => {
  let mock: MockBackend;
  let store: InstanceType<typeof EngineStore>;
  beforeEach(() => {
    mock = makeMock();
    TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: mock }, EngineStore] });
    store = TestBed.inject(EngineStore);
  });
  afterEach(() => {
    mock.dispose();
    TestBed.resetTestingModule();
  });

  it('loads the report; the engine is usable', async () => {
    await store.load();
    expect(store.report().value?.engine).toBe('docker');
    expect(store.usable()).toBe(true);
  });

  it('records a load error, keeps no value, and recovers on retry', async () => {
    vi.spyOn(mock, 'engineReport').mockRejectedValueOnce({ code: 'serviceUnavailable', message: 'down' });
    await store.load();
    expect(store.report().error?.message).toBe('down');
    expect(store.report().value).toBeNull();
    expect(store.usable()).toBe(false);
    await store.load();
    expect(store.report().error).toBeNull();
    expect(store.report().value).not.toBeNull();
  });

  it('is not usable without an engine or with a problem', async () => {
    const base = await mock.engineReport();
    vi.spyOn(mock, 'engineReport').mockResolvedValueOnce({ ...base, engine: null, problem: 'not installed' });
    await store.load();
    expect(store.usable()).toBe(false);
    vi.spyOn(mock, 'engineReport').mockResolvedValueOnce({ ...base, problem: 'daemon stopped' });
    await store.load();
    expect(store.usable()).toBe(false);
  });

  it('starts with volumes off and keeps the kinds in a stable order', () => {
    expect(store.selection().kinds).toEqual(['stoppedContainers', 'danglingImages', 'unusedImages', 'buildCache']);
    store.toggle('volumes', true);
    expect(store.selection().kinds.at(-1)).toBe('volumes');
    store.toggle('stoppedContainers', false);
    expect(store.selection().kinds).not.toContain('stoppedContainers');
  });

  it('normalises the age', () => {
    store.setOlderThanDays(7.9);
    expect(store.olderThanDays()).toBe(7);
    store.setOlderThanDays(-3);
    expect(store.olderThanDays()).toBe(0);
    store.setOlderThanDays(Number.NaN);
    expect(store.olderThanDays()).toBe(0);
  });

  it('previews without deleting, and any change of the selection invalidates the preview', async () => {
    const prune = vi.spyOn(mock, 'pruneEngine');
    expect(store.canPrune()).toBe(false);
    await store.runPreview();
    expect(store.currentPreview()?.totalBytes).toBeGreaterThan(0);
    expect(store.canPrune()).toBe(true);
    expect(prune).not.toHaveBeenCalled();
    store.toggle('volumes', true);
    expect(store.currentPreview()).toBeNull();
    expect(store.canPrune()).toBe(false);
    store.toggle('volumes', false);
    expect(store.currentPreview()).not.toBeNull();
    store.setOlderThanDays(30);
    expect(store.currentPreview()).toBeNull();
    expect(await store.prune()).toBe(false);
    expect(prune).not.toHaveBeenCalled();
  });

  it('cannot preview an empty selection', async () => {
    for (const k of [...store.selection().kinds]) store.toggle(k, false);
    expect(store.canPreview()).toBe(false);
    const spy = vi.spyOn(mock, 'previewEnginePrune');
    await store.runPreview();
    expect(spy).not.toHaveBeenCalled();
  });

  it('prunes the previewed selection, keeps the result and reloads the report', async () => {
    (mock as unknown as { runners: { currentJob: unknown }[] }).runners.forEach((r) => (r.currentJob = null));
    await store.load();
    await store.runPreview();
    const reload = vi.spyOn(mock, 'engineReport');
    expect(await store.prune()).toBe(true);
    expect(store.result()?.freed.length).toBe(4);
    expect(store.preview()).toBeNull();
    expect(reload).toHaveBeenCalledTimes(1);
  });

  it('shows a busy error from the prune and keeps the preview for a retry', async () => {
    await store.runPreview();
    vi.spyOn(mock, 'pruneEngine').mockRejectedValueOnce({ code: 'busy', message: 'a job is running' });
    expect(await store.prune()).toBe(false);
    expect(store.actionError()?.code).toBe('busy');
    expect(store.currentPreview()).not.toBeNull();
    expect(store.pruning()).toBe(false);
    store.dismissError();
    expect(store.actionError()).toBeNull();
  });

  it('records a failed preview', async () => {
    vi.spyOn(mock, 'previewEnginePrune').mockRejectedValueOnce({ code: 'internal', message: 'boom' });
    await store.runPreview();
    expect(store.actionError()?.message).toBe('boom');
    expect(store.previewing()).toBe(false);
    expect(store.currentPreview()).toBeNull();
  });
});
