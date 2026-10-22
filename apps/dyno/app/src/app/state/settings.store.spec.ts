import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../core/backend';
import { MockBackend } from '../core/mock-backend';
import { ThemeService } from '../core/theme';
import { Notifier } from '../shared/notifier';
import { SettingsStore } from './settings.store';

function setup() {
  const mock = new MockBackend({ latencyMs: 0, now: () => 1_700_000_000_000 });
  TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: mock }] });
  return { mock, store: TestBed.inject(SettingsStore), theme: TestBed.inject(ThemeService), notifier: TestBed.inject(Notifier) };
}

describe('SettingsStore', () => {
  it('load fetches settings, applies the theme and fills app info', async () => {
    const { store, mock, theme } = setup();
    const s = await mock.settings();
    s.theme = 'dark';
    vi.spyOn(mock, 'settings').mockResolvedValue(s);
    await store.load();
    expect(store.settings()?.theme).toBe('dark');
    expect(theme.preference()).toBe('dark');
    await new Promise((r) => setTimeout(r));
    expect(store.appInfo()?.os).toBe('macos');
  });
  it('load failure sets the error; app info failure is ignored', async () => {
    const { store, mock } = setup();
    vi.spyOn(mock, 'settings').mockRejectedValue({ code: 'internal', message: 'bad' });
    vi.spyOn(mock, 'appInfo').mockRejectedValue(new Error('x'));
    await store.load();
    expect(store.error()).toEqual({ code: 'internal', message: 'bad' });
    expect(store.settings()).toBeNull();
    await new Promise((r) => setTimeout(r));
    expect(store.appInfo()).toBeNull();
  });
  it('applyTheme previews without saving', () => {
    const { store, theme } = setup();
    store.applyTheme('light');
    expect(theme.preference()).toBe('light');
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
  it('save stores what the engine returns, applies the theme and toasts', async () => {
    const { store, mock, theme, notifier } = setup();
    const s = structuredClone(await mock.settings());
    s.theme = 'light';
    expect(await store.save(s)).toBe(true);
    expect(store.settings()?.theme).toBe('light');
    expect(theme.preference()).toBe('light');
    expect(store.saving()).toBe(false);
    expect(notifier.toasts().at(-1)?.text).toBe('Settings saved');
  });
  it('save without a message stays quiet', async () => {
    const { store, mock, notifier } = setup();
    expect(await store.save(structuredClone(await mock.settings()), null)).toBe(true);
    expect(notifier.toasts()).toEqual([]);
  });
  it('save rejects invalid settings with the engine message', async () => {
    const { store, mock, notifier } = setup();
    const s = structuredClone(await mock.settings());
    s.sampling.intervalMs = 10;
    expect(await store.save(s)).toBe(false);
    expect(store.saving()).toBe(false);
    expect(store.settings()).toBeNull();
    expect(notifier.toasts().at(-1)).toMatchObject({ tone: 'bad', text: expect.stringContaining('250 ms') });
  });
});
