import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../core/backend';
import { MockBackend } from '../core/mock-backend';
import { Notifier } from '../shared/notifier';
import { SettingsStore } from './settings.store';

function setup() {
  const mock = new MockBackend({ latencyMs: 0, now: () => 1_700_000_000_000 });
  TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: mock }] });
  return { mock, store: TestBed.inject(SettingsStore), notifier: TestBed.inject(Notifier) };
}

describe('SettingsStore', () => {
  it('load fetches the settings (the appearance is the shell’s, not part of them)', async () => {
    const { store, mock } = setup();
    const s = await mock.settings();
    vi.spyOn(mock, 'settings').mockResolvedValue(s);
    await store.load();
    expect(store.settings()).toEqual(s);
    expect(store.error()).toBeNull();
  });
  it('load failure sets the error', async () => {
    const { store, mock } = setup();
    vi.spyOn(mock, 'settings').mockRejectedValue({ code: 'internal', message: 'bad' });
    await store.load();
    expect(store.error()).toEqual({ code: 'internal', message: 'bad' });
    expect(store.settings()).toBeNull();
  });
  it('loadPlugins fills or fails', async () => {
    const { store, mock } = setup();
    await store.loadPlugins();
    expect(store.pluginsLoaded()).toBe(true);
    expect(store.plugins().length).toBeGreaterThan(0);
    vi.spyOn(mock, 'plugins').mockRejectedValue({ code: 'busy', message: 'b' });
    await store.loadPlugins();
    expect(store.pluginsError()).toEqual({ code: 'busy', message: 'b' });
  });
  it('save stores what the engine returns and tells the user', async () => {
    const { store, mock, notifier } = setup();
    const s = structuredClone(await mock.settings());
    s.sampling.intervalMs = 2000;
    expect(await store.save(s)).toBe(true);
    expect(store.settings()?.sampling.intervalMs).toBe(2000);
    expect(store.saving()).toBe(false);
    expect(notifier.recent().at(-1)?.text).toBe('Settings saved');
  });
  it('save without a message stays quiet', async () => {
    const { store, mock, notifier } = setup();
    expect(await store.save(structuredClone(await mock.settings()), null)).toBe(true);
    expect(notifier.recent()).toEqual([]);
  });
  it('save rejects invalid settings with the engine message', async () => {
    const { store, mock, notifier } = setup();
    const s = structuredClone(await mock.settings());
    s.sampling.intervalMs = 10;
    expect(await store.save(s)).toBe(false);
    expect(store.saving()).toBe(false);
    expect(store.settings()).toBeNull();
    expect(notifier.recent().at(-1)).toMatchObject({ tone: 'bad', text: expect.stringContaining('250 ms') });
  });
  it('a failed save keeps the error, and the next good save clears it', async () => {
    const { store, mock } = setup();
    await store.load();
    const s = structuredClone(await mock.settings());
    s.sampling.intervalMs = 10;
    expect(await store.save(s)).toBe(false);
    expect(store.saveError()?.message).toContain('250 ms');
    expect(await store.save(structuredClone(await mock.settings()))).toBe(true);
    expect(store.saveError()).toBeNull();
  });
});
