import { TestBed } from '@angular/core/testing';
import type { PitwallBackend } from '../../core/backend';
import { BACKEND } from '../../core/backend';
import { MockBackend } from '../../core/mock-backend';
import { Toasts } from '../../core/toasts';
import { demoSettings } from '../../core/mock-data';
import { SETTINGS_JOURNAL, SETTINGS_JOURNAL_KEY, SETTINGS_JOURNAL_TTL_MS, SETTINGS_SAVE_DELAY, SettingsStore, editSettings, refusalText, undoText } from './settings.store';

describe('editSettings', () => {
  it('merges values into one section and leaves the others alone', () => {
    const s = demoSettings();
    const next = editSettings(s, 'server', { port: 12000 });
    expect(next.server.port).toBe(12000);
    expect(next.server.maxConcurrent).toBe(s.server.maxConcurrent);
    expect(next.network).toEqual(s.network);
    expect(s.server.port).toBe(11500);
  });

  it('makes the preset custom when a memory value changes, but not when the preset itself is set', () => {
    const s = demoSettings();
    expect(editSettings(s, 'memory', { maxHotModels: 2 }).memory.preset).toBe('custom');
    expect(editSettings(s, 'memory', { preset: 'performance' }).memory.preset).toBe('performance');
    expect(editSettings(s, 'general', { onQuit: 'ask' }).memory.preset).toBe('balanced');
  });
});

describe('SettingsStore', () => {
  const PORT = { id: 'srv-port', label: 'Port' };
  const CONC = { id: 'srv-conc', label: 'Requests at once' };
  const QUIT = { id: 'gen-quit', label: 'When you quit Pitwall' };
  const HOT = { id: 'mem-hot', label: 'Hot models at once' };
  const RETAIN = { id: 'prv-ret', label: 'Keep metrics and history for' };
  const OFFLINE = { id: 'network.offline', label: 'Offline mode' };
  let journal: Map<string, string>;

  function setup(backend: PitwallBackend = new MockBackend(), delay = 5) {
    journal = new Map();
    const storage = { getItem: (k: string) => journal.get(k) ?? null, setItem: (k: string, v: string) => void journal.set(k, v), removeItem: (k: string) => void journal.delete(k) } as Storage;
    TestBed.configureTestingModule({
      providers: [SettingsStore, { provide: BACKEND, useValue: backend }, { provide: SETTINGS_SAVE_DELAY, useValue: delay }, { provide: SETTINGS_JOURNAL, useValue: () => storage }],
    });
    return { store: TestBed.inject(SettingsStore), toasts: TestBed.inject(Toasts), backend };
  }

  afterEach(() => vi.useRealTimers());

  it('is empty and clean before loading', () => {
    const { store } = setup();
    expect(store.saved()).toBeNull();
    expect(store.draft()).toBeNull();
    expect(store.dirty()).toBe(false);
    expect(store.refusals()).toEqual([]);
  });

  it('loads settings, a separate draft copy', async () => {
    const { store } = setup();
    const p = store.load();
    expect(store.loading()).toBe(true);
    await p;
    expect(store.loading()).toBe(false);
    expect(store.saved()?.server.port).toBe(11500);
    expect(store.draft()).toEqual(store.saved());
    expect(store.draft()).not.toBe(store.saved());
    expect(store.error()).toBeNull();
  });

  it('reports a load error', async () => {
    const { store } = setup(new MockBackend({ failing: ['settings'] }));
    await store.load();
    expect(store.error()?.code).toBe('serverUnavailable');
    expect(store.draft()).toBeNull();
    expect(store.loading()).toBe(false);
  });

  it('ignores edits before load', () => {
    const { store } = setup();
    store.edit(PORT, 'server', { port: 2000 });
    store.revert();
    expect(store.draft()).toBeNull();
  });

  it('saves a switch or a choice at once, without waiting for the pause', async () => {
    const { store, backend, toasts } = setup(new MockBackend(), 60_000);
    await store.load();
    const save = vi.spyOn(backend, 'saveSettings');
    store.edit(OFFLINE, 'network', { offline: true });
    expect(store.draft()?.network.offline).toBe(true);
    expect(store.saving()).toBe(true);
    await vi.waitFor(() => expect(store.saved()?.network.offline).toBe(true));
    expect(save).toHaveBeenCalledTimes(1);
    expect(store.saving()).toBe(false);
    expect(toasts.toasts()).toEqual([]);
  });

  it('saves a typed number after the pause, once for a burst of keystrokes', async () => {
    vi.useFakeTimers();
    const { store, backend } = setup(new MockBackend(), 500);
    await store.load();
    const save = vi.spyOn(backend, 'saveSettings');
    store.edit(CONC, 'server', { maxConcurrent: 1 }, 'typed');
    await vi.advanceTimersByTimeAsync(300);
    store.edit(CONC, 'server', { maxConcurrent: 12 }, 'typed');
    await vi.advanceTimersByTimeAsync(499);
    expect(save).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(1);
    await vi.waitFor(() => expect(store.saved()?.server.maxConcurrent).toBe(12));
    expect(save).toHaveBeenCalledTimes(1);
  });

  it('saves a typed number at close when that comes before the pause, and the close waits for it', async () => {
    vi.useFakeTimers();
    const { store, backend } = setup(new MockBackend(), 500);
    await store.load();
    store.edit(CONC, 'server', { maxConcurrent: 12 }, 'typed');
    const verdict = store.closeVerdict();
    expect(verdict.kind).toBe('wait');
    await (verdict as { done: Promise<unknown> }).done;
    expect((await backend.settings()).server.maxConcurrent).toBe(12);
    expect(store.closeVerdict()).toEqual({ kind: 'close' });
    // The pause passing later sends nothing more.
    const save = vi.spyOn(backend, 'saveSettings');
    await vi.advanceTimersByTimeAsync(1000);
    expect(save).not.toHaveBeenCalled();
  });

  it('a typed value does not redraw its own field mid-word; a commit does', async () => {
    const { store } = setup(new MockBackend(), 60_000);
    await store.load();
    store.edit(CONC, 'server', { maxConcurrent: 3 }, 'typed');
    expect(store.draft()?.server.maxConcurrent).toBe(4);
    store.edit(CONC, 'server', { maxConcurrent: 3 }, 'now');
    expect(store.draft()?.server.maxConcurrent).toBe(3);
    await store.flush();
  });

  it('saves edits in the order made, each over what the server has, and keeps a newer edit made meanwhile', async () => {
    const { store, backend } = setup();
    await store.load();
    store.edit(PORT, 'server', { port: 12001 });
    store.edit(PORT, 'server', { port: 12002 });
    store.edit(QUIT, 'general', { onQuit: 'ask' });
    expect(store.draft()?.server.port).toBe(12002);
    expect(await store.flush()).toBe(true);
    const now = await backend.settings();
    expect(now.server.port).toBe(12002);
    expect(now.general.onQuit).toBe('ask');
    expect(store.dirty()).toBe(false);
  });

  it('names the refused setting with the server’s reason and keeps it in the form, without a toast', async () => {
    const { store, toasts } = setup();
    await store.load();
    store.edit(PORT, 'server', { port: 80 });
    expect(await store.flush()).toBe(false);
    expect(store.refusals()).toHaveLength(1);
    const r = store.refusals()[0]!;
    expect(refusalText(r)).toBe('Port could not be saved: The port must be between 1024 and 65535.');
    expect(undoText(r)).toBe('Undo port change');
    expect(undoText({ ...r, field: { id: 'x', label: 'LAN port' } })).toBe('Undo LAN port change');
    expect(store.draft()?.server.port).toBe(80);
    expect(store.saving()).toBe(false);
    expect(toasts.toasts()).toEqual([]);
  });

  it('one refused setting does not block the others: they keep saving, and the refused one is not sent again', async () => {
    const { store, backend } = setup();
    await store.load();
    store.edit(PORT, 'server', { port: 80 });
    await store.flush();
    const save = vi.spyOn(backend, 'saveSettings');
    store.edit(QUIT, 'general', { onQuit: 'ask' });
    store.edit(HOT, 'memory', { maxHotModels: 2 });
    expect(await store.flush()).toBe(false);
    expect(save).toHaveBeenCalledTimes(2);
    for (const [body] of save.mock.calls) expect(body.server.port).toBe(11500);
    const now = await backend.settings();
    expect(now.general.onQuit).toBe('ask');
    expect(now.memory.maxHotModels).toBe(2);
    // The form still shows the refused port beside the saved values.
    expect(store.draft()?.server.port).toBe(80);
    expect(store.refusals().map((r) => r.field.id)).toEqual(['srv-port']);
  });

  it('a refused setting clears when edited again, is undone by its own Undo, and Retry sends it once more', async () => {
    const backend = new MockBackend();
    const { store } = setup(backend);
    await store.load();
    store.edit(PORT, 'server', { port: 80 });
    store.edit(RETAIN, 'privacy', { metricsRetentionDays: 400 });
    vi.spyOn(backend, 'saveSettings').mockImplementation(async (s) => {
      if (s.privacy.metricsRetentionDays > 365) throw { code: 'badRequest', message: 'Keep history for at most 365 days.' };
      if (s.server.port < 1024) throw { code: 'badRequest', message: 'The port must be between 1024 and 65535.' };
      return s;
    });
    await store.flush();
    expect(store.refusals().map((r) => r.field.id)).toEqual(['srv-port', 'prv-ret']);
    store.revert('prv-ret');
    expect(store.refusals().map((r) => r.field.id)).toEqual(['srv-port']);
    expect(store.draft()?.privacy.metricsRetentionDays).toBe(30);
    expect(store.draft()?.server.port).toBe(80);
    // Retry: refused again (the server still says no), then edited to a good value.
    expect(await store.retry('srv-port')).toBe(false);
    expect(await store.retry('nothing-there')).toBe(true);
    store.edit(PORT, 'server', { port: 12000 });
    expect(store.refusals()).toEqual([]);
    expect(await store.flush()).toBe(true);
    expect(store.saved()?.server.port).toBe(12000);
  });

  it('Retry saves a failed setting once the server answers, and Undo all puts every edit back', async () => {
    const backend = new MockBackend();
    const { store } = setup(backend);
    await store.load();
    vi.spyOn(backend, 'saveSettings').mockRejectedValueOnce({ code: 'serverUnavailable', message: 'down' });
    store.edit(QUIT, 'general', { onQuit: 'ask' });
    expect(await store.flush()).toBe(false);
    expect(store.refusals()).toHaveLength(1);
    expect(await store.retry()).toBe(true);
    expect(store.refusals()).toEqual([]);
    expect((await backend.settings()).general.onQuit).toBe('ask');
    store.edit(PORT, 'server', { port: 80 });
    await store.flush();
    store.edit(CONC, 'server', { maxConcurrent: 9 }, 'typed');
    store.revert();
    expect(store.refusals()).toEqual([]);
    expect(store.draft()).toEqual(store.saved());
    expect(await store.flush()).toBe(true);
  });

  it('a refusal of an edit superseded while on its way is moot', async () => {
    const backend = new MockBackend();
    const { store } = setup(backend);
    await store.load();
    let release!: () => void;
    const gate = new Promise<void>((r) => (release = r));
    vi.spyOn(backend, 'saveSettings').mockImplementationOnce(async () => {
      await gate;
      throw { code: 'badRequest', message: 'no' };
    });
    store.edit(PORT, 'server', { port: 80 });
    await Promise.resolve();
    store.edit(PORT, 'server', { port: 12000 });
    release();
    expect(await store.flush()).toBe(true);
    expect(store.refusals()).toEqual([]);
    expect(store.saved()?.server.port).toBe(12000);
  });

  it('closing says which settings were not saved, and why', async () => {
    const { store } = setup();
    await store.load();
    expect(store.closeVerdict()).toEqual({ kind: 'close' });
    store.edit(PORT, 'server', { port: 80 });
    await store.flush();
    expect(store.closeVerdict()).toEqual({ kind: 'warn', message: 'Port was not saved: The port must be between 1024 and 65535.' });
  });

  it('writes unsent edits down and the next page sends them; a sent edit is crossed out', async () => {
    const backend = new MockBackend();
    const { store } = setup(backend, 60_000);
    await store.load();
    store.edit(CONC, 'server', { maxConcurrent: 7 }, 'typed');
    const written = journal.get(SETTINGS_JOURNAL_KEY)!;
    expect(JSON.parse(written).edits[0].field).toEqual(CONC);
    // The window went without waiting: a new page (a new store) over the same storage sends it.
    TestBed.resetTestingModule();
    const again = setup(backend, 60_000);
    journal.set(SETTINGS_JOURNAL_KEY, written);
    await again.store.load();
    await again.store.flush();
    expect((await backend.settings()).server.maxConcurrent).toBe(7);
    expect(journal.has(SETTINGS_JOURNAL_KEY)).toBe(false);
  });

  it('ignores a stale, malformed or foreign journal', async () => {
    const backend = new MockBackend();
    const { store } = setup(backend);
    const save = vi.spyOn(backend, 'saveSettings');
    for (const raw of [
      JSON.stringify({ at: Date.now() - SETTINGS_JOURNAL_TTL_MS - 1, edits: [{ field: CONC, section: 'server', values: { maxConcurrent: 7 } }] }),
      JSON.stringify({ at: Date.now(), edits: [{ field: CONC, section: 'nope', values: {} }, { field: { id: 1 }, section: 'server', values: {} }, { field: CONC, section: 'server', values: [] }, null] }),
      JSON.stringify({ at: Date.now(), edits: 'x' }),
      '{broken',
      'null',
    ]) {
      journal.set(SETTINGS_JOURNAL_KEY, raw);
      await store.load();
      await store.flush();
    }
    expect(save).not.toHaveBeenCalled();
  });

  it('survives storage that refuses', async () => {
    TestBed.configureTestingModule({
      providers: [
        SettingsStore,
        { provide: BACKEND, useValue: new MockBackend() },
        {
          provide: SETTINGS_JOURNAL,
          useValue: () => {
            throw new Error('denied');
          },
        },
      ],
    });
    const store = TestBed.inject(SettingsStore);
    await store.load();
    store.edit(QUIT, 'general', { onQuit: 'ask' });
    expect(await store.flush()).toBe(true);
  });

  it('the default journal is the origin storage', () => {
    TestBed.configureTestingModule({});
    expect(TestBed.inject(SETTINGS_JOURNAL)()).toBe(globalThis.localStorage);
  });

  it('applies a preset after the edits made before it, and shows its memory values', async () => {
    const { store, toasts } = setup();
    await store.load();
    store.edit(QUIT, 'general', { onQuit: 'ask' }, 'typed');
    await store.applyPreset('lowMemory');
    expect(store.saved()?.general.onQuit).toBe('ask');
    expect(store.draft()?.memory.preset).toBe('lowMemory');
    expect(store.draft()?.memory.maxHotModels).toBe(1);
    expect(store.draft()?.general.onQuit).toBe('ask');
    expect(toasts.toasts().at(-1)?.message).toBe('Low memory preset applied.');
  });

  it('a preset chosen while a save is on its way is queued after it, not dropped', async () => {
    const backend = new MockBackend();
    const { store } = setup(backend);
    await store.load();
    let release!: () => void;
    const gate = new Promise<void>((r) => (release = r));
    const real = backend.saveSettings.bind(backend);
    const order: string[] = [];
    vi.spyOn(backend, 'saveSettings').mockImplementation(async (s) => {
      await gate;
      order.push('save');
      return real(s);
    });
    const preset = vi.spyOn(backend, 'applyPreset');
    store.edit(HOT, 'memory', { maxHotModels: 5 });
    const first = store.applyPreset('performance');
    const second = store.applyPreset('lowMemory');
    expect(preset).not.toHaveBeenCalled();
    release();
    await Promise.all([first, second]);
    expect(order).toEqual(['save']);
    expect(preset.mock.calls.map(([p]) => p)).toEqual(['performance', 'lowMemory']);
    expect(store.saved()?.memory.preset).toBe('lowMemory');
    expect(store.saving()).toBe(false);
  });

  it('a preset replaces a refused memory edit', async () => {
    const backend = new MockBackend();
    const { store } = setup(backend);
    await store.load();
    vi.spyOn(backend, 'saveSettings').mockRejectedValueOnce({ code: 'badRequest', message: 'too many' });
    store.edit(HOT, 'memory', { maxHotModels: 15 });
    store.edit(PORT, 'server', { port: 80 });
    await store.flush();
    expect(store.refusals().map((r) => r.field.id)).toEqual(['mem-hot', 'srv-port']);
    await store.applyPreset('balanced');
    expect(store.refusals().map((r) => r.field.id)).toEqual(['srv-port']);
    expect(store.draft()?.memory.preset).toBe('balanced');
  });

  it('applies a preset before the draft exists by adopting the saved settings', async () => {
    const { store } = setup();
    await store.applyPreset('performance');
    expect(store.draft()?.memory.preset).toBe('performance');
  });

  it('reports a failing preset', async () => {
    const { store, toasts } = setup(new MockBackend({ failing: ['applyPreset'] }));
    await store.load();
    await store.applyPreset('balanced');
    expect(store.saving()).toBe(false);
    expect(toasts.toasts().at(-1)?.tone).toBe('bad');
  });
});

describe('SettingsStore caching', () => {
  function setup(backend: PitwallBackend = new MockBackend()) {
    TestBed.configureTestingModule({ providers: [SettingsStore, { provide: BACKEND, useValue: backend }] });
    return { store: TestBed.inject(SettingsStore), toasts: TestBed.inject(Toasts), backend };
  }

  it('loads the cache figures and keeps the last good ones when a refresh fails', async () => {
    const backend = new MockBackend();
    const { store } = setup(backend);
    await store.loadCacheInfo();
    expect(store.cacheInfo()!.responses.entries).toBeGreaterThan(0);
    vi.spyOn(backend, 'cacheInfo').mockRejectedValueOnce({ code: 'serverUnavailable', message: 'down' });
    await store.loadCacheInfo();
    expect(store.cacheError()?.message).toBe('down');
    expect(store.cacheInfo()).not.toBeNull();
    await store.loadCacheInfo();
    expect(store.cacheError()).toBeNull();
  });

  it('clears a cache, says how many entries went and refreshes the figures', async () => {
    const { store, toasts } = setup();
    await store.loadCacheInfo();
    const n = store.cacheInfo()!.embeddings.entries;
    expect(await store.clearCache({ kind: 'embeddings' })).toBe(n);
    expect(toasts.toasts().at(-1)?.message).toBe(`Removed ${n} cached entries.`);
    expect(store.cacheInfo()!.embeddings.entries).toBe(0);
    expect(store.clearing()).toBe(false);
    expect(await store.clearCache({ kind: 'embeddings' })).toBe(0);
    expect(toasts.toasts().at(-1)?.message).toContain('already empty');
  });

  it('reports a failed clear as an error toast and stays usable', async () => {
    const { store, toasts } = setup(new MockBackend({ failing: ['clearCache'] }));
    expect(await store.clearCache({ kind: 'all' })).toBeUndefined();
    expect(toasts.toasts().at(-1)?.tone).toBe('bad');
    expect(store.clearing()).toBe(false);
  });

  it('edits the cache section like any other and marks the form dirty until it is saved', async () => {
    const { store } = setup();
    await store.load();
    store.edit({ id: 'cache-responses-expiry', label: 'Cached answers expiry' }, 'cache', { responsesTtlSecs: 3600 });
    expect(store.draft()!.cache.responsesTtlSecs).toBe(3600);
    expect(store.draft()!.memory.preset).toBe('balanced');
    expect(store.dirty()).toBe(true);
  });
});
