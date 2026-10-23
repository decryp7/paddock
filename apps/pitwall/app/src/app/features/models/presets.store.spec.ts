import { TestBed } from '@angular/core/testing';
import type { PitwallBackend } from '../../core/backend';
import { BACKEND } from '../../core/backend';
import type { PresetDraft } from '../../core/api.types';
import { MockBackend, type MockOptions } from '../../core/mock-backend';
import { Toasts } from '../../core/toasts';
import { PREVIEW_DELAY_MS, PresetsStore } from './presets.store';

function setup(backend: PitwallBackend = new MockBackend()) {
  TestBed.configureTestingModule({ providers: [PresetsStore, { provide: BACKEND, useValue: backend }] });
  return { store: TestBed.inject(PresetsStore), toasts: TestBed.inject(Toasts), backend };
}

const failing = (...methods: (keyof PitwallBackend)[]) => new MockBackend({ failing: methods } as MockOptions);
const draft = (alias: string, target = 'qwen3-8b-instruct-q4_k_m'): PresetDraft => ({ alias, target, context: null, temperature: null, topP: null, maxTokens: null, systemPrompt: null, cacheResponses: false, keepWarm: false });

describe('PresetsStore', () => {
  afterEach(() => vi.useRealTimers());

  it('loads the presets with where each stands', async () => {
    const { store } = setup();
    const done = store.load();
    expect(store.loading()).toBe(true);
    await done;
    expect(store.loaded()).toBe(true);
    expect(store.entries().map((e) => e.preset.alias)).toEqual(['debrief-explain', 'legacy-explain', 'pitcrew-triage', 'spotter-summarise']);
    expect(store.entries().find((e) => e.preset.alias === 'legacy-explain')?.status.state).toBe('stale');
    expect(store.broken()).toBe(1);
  });

  it('shows a load error', async () => {
    const { store } = setup(failing('presets'));
    await store.load();
    expect(store.error()?.code).toBe('serverUnavailable');
    expect(store.loaded()).toBe(false);
  });

  it('creates a preset, closes the editor and reloads', async () => {
    const { store, toasts } = setup();
    await store.load();
    store.openEditor(null);
    expect(store.editor()).toEqual({ editing: null });
    expect(await store.save(draft('fresh-one'))).toBe(true);
    expect(store.editor()).toBeNull();
    expect(store.entries().some((e) => e.preset.alias === 'fresh-one')).toBe(true);
    expect(toasts.toasts().at(-1)?.message).toContain('Created fresh-one');
  });

  it('updates and renames the edited preset', async () => {
    const { store, toasts } = setup();
    await store.load();
    store.openEditor('spotter-summarise');
    expect(await store.save(draft('spotter-short'))).toBe(true);
    const names = store.entries().map((e) => e.preset.alias);
    expect(names).toContain('spotter-short');
    expect(names).not.toContain('spotter-summarise');
    expect(toasts.toasts().at(-1)?.message).toContain('Saved spotter-short');
  });

  it('keeps the editor open with the engine\'s message when saving fails', async () => {
    const { store } = setup();
    await store.load();
    store.openEditor(null);
    expect(await store.save(draft('debrief-explain'))).toBe(false);
    expect(store.saveError()?.message).toContain('already exists');
    expect(store.editor()).not.toBeNull();
    expect(store.saving()).toBe(false);
    expect(await store.save(draft('tiny', 'gone-model'))).toBe(false);
    expect(store.saveError()?.message).toContain('not installed');
    store.closeEditor();
    expect(store.editor()).toBeNull();
    expect(store.saveError()).toBeNull();
    expect(await store.save(draft('x'))).toBe(false);
  });

  it('does not save twice at once', async () => {
    const { store } = setup(new MockBackend({ latencyMs: 5 }));
    await store.load();
    store.openEditor(null);
    const first = store.save(draft('only-once'));
    expect(await store.save(draft('only-twice'))).toBe(false);
    expect(await first).toBe(true);
    expect(store.entries().filter((e) => e.preset.alias.startsWith('only')).length).toBe(1);
  });

  it('deletes a preset, and closes the editor when it was the one being edited', async () => {
    const { store, toasts } = setup();
    await store.load();
    store.openEditor('legacy-explain');
    expect(await store.remove('legacy-explain')).toBe(true);
    expect(store.entries().some((e) => e.preset.alias === 'legacy-explain')).toBe(false);
    expect(store.editor()).toBeNull();
    expect(toasts.toasts().at(-1)?.message).toContain('Deleted legacy-explain');
    expect(store.deleting()).toEqual([]);
  });

  it('reports a failed delete and keeps the list', async () => {
    const { store, toasts } = setup();
    await store.load();
    expect(await store.remove('nothing-here')).toBe(false);
    expect(toasts.toasts().at(-1)?.tone).toBe("bad");
    expect(store.entries().length).toBe(4);
    expect(store.deleting()).toEqual([]);
  });

  it('ignores a second delete of the same alias while one runs', async () => {
    const { store } = setup(new MockBackend({ latencyMs: 5 }));
    await store.load();
    const first = store.remove('legacy-explain');
    expect(await store.remove('legacy-explain')).toBe(false);
    expect(await first).toBe(true);
  });

  it('asks the engine for a preview only after typing pauses, and keeps the latest answer', async () => {
    vi.useFakeTimers();
    const { store, backend } = setup();
    const spy = vi.spyOn(backend, 'resolvePreset');
    store.openEditor(null);
    store.schedulePreview(draft('a'));
    store.schedulePreview(draft('ab'));
    store.schedulePreview(draft('abc', 'auto:chat,fast'));
    expect(spy).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(PREVIEW_DELAY_MS);
    expect(spy).toHaveBeenCalledTimes(1);
    expect(spy).toHaveBeenCalledWith(expect.objectContaining({ alias: 'abc' }), null);
    expect(store.preview()?.problem).toBeNull();
    expect(store.preview()?.status?.state).toBe('ready');
    expect(store.previewing()).toBe(false);
  });

  it('tells the engine which preset is being edited, so it is not its own duplicate', async () => {
    vi.useFakeTimers();
    const { store, backend } = setup();
    const spy = vi.spyOn(backend, 'resolvePreset');
    store.openEditor('pitcrew-triage');
    store.schedulePreview(draft('pitcrew-triage', 'auto:chat'));
    await vi.advanceTimersByTimeAsync(PREVIEW_DELAY_MS);
    expect(spy).toHaveBeenCalledWith(expect.anything(), 'pitcrew-triage');
    expect(store.preview()?.problem).toBeNull();
  });

  it('drops an answer that was overtaken, and a pending preview when the editor closes', async () => {
    vi.useFakeTimers();
    const { store, backend } = setup();
    const spy = vi.spyOn(backend, 'resolvePreset');
    store.openEditor(null);
    store.schedulePreview(draft('slow'));
    await vi.advanceTimersByTimeAsync(PREVIEW_DELAY_MS - 1);
    store.closeEditor();
    await vi.advanceTimersByTimeAsync(PREVIEW_DELAY_MS);
    expect(spy).not.toHaveBeenCalled();
    expect(store.preview()).toBeNull();
    // A request in flight when a newer draft is scheduled does not overwrite it.
    store.openEditor(null);
    store.schedulePreview(draft('first', 'gone-model'));
    await vi.advanceTimersByTimeAsync(PREVIEW_DELAY_MS);
    store.schedulePreview(draft('second'));
    await vi.advanceTimersByTimeAsync(PREVIEW_DELAY_MS);
    expect(store.preview()?.problem).toBeNull();
  });

  it('shows a failed preview as a problem instead of crashing', async () => {
    vi.useFakeTimers();
    const { store } = setup(failing('resolvePreset'));
    store.openEditor(null);
    store.schedulePreview(draft('x'));
    await vi.advanceTimersByTimeAsync(PREVIEW_DELAY_MS);
    expect(store.preview()?.problem).toContain('not responding');
    expect(store.preview()?.status).toBeNull();
    expect(store.previewing()).toBe(false);
  });
});
