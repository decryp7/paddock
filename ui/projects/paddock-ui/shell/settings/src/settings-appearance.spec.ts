import { signal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { AppearanceStore, BrowserAppearanceBackend, DEFAULT_APPEARANCE, provideAppearance, type Appearance, type AppearanceStorage } from '@decryp7/paddock-ui/shell';
import { APPEARANCE_SECTION } from '../generated/settings';
import { MemorySettingsBackend, SettingsBackend } from './settings-backend';
import { SettingsAppearanceBackend, settingsAppearanceBackend } from './settings-appearance';

class MemoryStorage implements AppearanceStorage {
  readonly map = new Map<string, string>();
  getItem(k: string): string | null {
    return this.map.get(k) ?? null;
  }
  setItem(k: string, v: string): void {
    this.map.set(k, v);
  }
}

const flush = async (): Promise<void> => {
  for (let i = 0; i < 6; i++) await Promise.resolve();
};

describe('SettingsAppearanceBackend (plan S15 over S10)', () => {
  it('theme and density follow the engine; the rest stays in the window', async () => {
    const settings = new MemorySettingsBackend();
    const local = new BrowserAppearanceBackend({ storage: new MemoryStorage(), channel: null });
    const backend = new SettingsAppearanceBackend(settings, local);
    const heard: Appearance[] = [];
    const stop = backend.listen((a) => heard.push(a));
    await flush();
    const engine = settings.values(APPEARANCE_SECTION) ?? {};
    expect(backend.load().theme).toBe(engine['theme']);

    // A change made here goes to the engine (theme, density) and stays local (contrast).
    backend.save({ ...backend.load(), theme: 'light', contrast: 'more' });
    await flush();
    expect(settings.values(APPEARANCE_SECTION)).toMatchObject({ theme: 'light' });
    expect(local.load()).toMatchObject({ theme: 'light', contrast: 'more' });

    // A change made elsewhere (the Settings window, another window) arrives from the engine.
    await settings.set(APPEARANCE_SECTION, { density: 'compact' });
    await flush();
    expect(heard.at(-1)).toMatchObject({ theme: 'light', contrast: 'more', density: 'compact' });
    const count = heard.length;
    stop();
    await settings.set(APPEARANCE_SECTION, { density: 'comfortable' });
    await flush();
    expect(heard).toHaveLength(count);
  });

  it('a contrast-only change does not write the engine; failures are reported, never thrown', async () => {
    const settings = new MemorySettingsBackend();
    const writes: unknown[] = [];
    const spy = Object.assign(Object.create(settings) as SettingsBackend, {
      set: (s: string, c: unknown) => (writes.push([s, c]), Promise.reject(new Error('engine gone'))),
      listen: () => Promise.reject(new Error('no events')),
    });
    const warnings: string[] = [];
    const backend = new SettingsAppearanceBackend(spy, new BrowserAppearanceBackend({ storage: new MemoryStorage(), channel: null }), (s) => s(), (m) => warnings.push(m));
    backend.save({ ...DEFAULT_APPEARANCE, contrast: 'more' });
    expect(writes).toEqual([]);
    backend.save({ ...DEFAULT_APPEARANCE, contrast: 'more', theme: 'dark' });
    backend.listen(() => undefined);
    await flush();
    expect(writes).toHaveLength(1);
    expect(warnings).toEqual([
      '[paddock-settings] the appearance could not be saved: engine gone',
      '[paddock-settings] the appearance settings could not be read: no events',
    ]);
  });

  it('asks the engine nothing before the gate opens (start-up sends only the contract’s calls)', async () => {
    const settings = new MemorySettingsBackend();
    let asked = 0;
    const counting = Object.assign(Object.create(settings) as SettingsBackend, {
      overview: () => (asked++, settings.overview()),
      listen: (h: Parameters<SettingsBackend['listen']>[0]) => (asked++, settings.listen(h)),
    });
    const ready = signal(false);
    TestBed.configureTestingModule({
      providers: [
        { provide: SettingsBackend, useValue: counting },
        provideAppearance({
          accent: 'pitwall',
          root: () => document.createElement('html'),
          backend: settingsAppearanceBackend({ local: () => new BrowserAppearanceBackend({ storage: new MemoryStorage(), channel: null }), gate: () => ready }),
        }),
      ],
    });
    const store = TestBed.inject(AppearanceStore);
    TestBed.tick();
    await flush();
    expect(asked).toBe(0);
    ready.set(true);
    TestBed.tick();
    await flush();
    expect(asked).toBe(2);
    await settings.set(APPEARANCE_SECTION, { theme: 'light' });
    await flush();
    expect(store.theme()).toBe('light');
  });

  it('without a gate it starts at once and keeps its copy in localStorage by default', async () => {
    const settings = new MemorySettingsBackend();
    await settings.set(APPEARANCE_SECTION, { theme: 'dark' });
    TestBed.configureTestingModule({
      providers: [{ provide: SettingsBackend, useValue: settings }, provideAppearance({ accent: 'pitwall', root: () => document.createElement('html'), backend: settingsAppearanceBackend() })],
    });
    const store = TestBed.inject(AppearanceStore);
    await flush();
    expect(store.theme()).toBe('dark');
    expect(JSON.parse(localStorage.getItem('paddock.appearance') ?? '{}')).toMatchObject({ theme: 'dark' });
    localStorage.clear();
  });
});
