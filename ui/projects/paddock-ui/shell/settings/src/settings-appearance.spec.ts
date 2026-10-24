import { createEnvironmentInjector, EnvironmentInjector, signal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { AppearanceStore, BrowserAppearanceBackend, DEFAULT_APPEARANCE, provideAppearance, type AppearanceStorage } from '@decryp7/paddock-ui/shell';
import { APPEARANCE_SECTION } from '../generated/settings';
import { MemorySettingsBackend, SettingsBackend } from './settings-backend';
import { provideSettingsAppearance, SettingsAppearanceBackend, settingsAppearanceBackend } from './settings-appearance';

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
  for (let i = 0; i < 8; i++) await Promise.resolve();
};
const copy = () => new BrowserAppearanceBackend({ storage: new MemoryStorage(), channel: null });

describe('SettingsAppearanceBackend (the engine is the one source of the look)', () => {
  it('saves the whole look to the engine and keeps a copy for the next start', async () => {
    const settings = new MemorySettingsBackend();
    const local = copy();
    const backend = new SettingsAppearanceBackend(settings, local);
    backend.save({ theme: 'light', contrast: 'more', motion: 'reduce', density: 'compact', pack: 'sd.theme.warm-paper' });
    await flush();
    expect(settings.values(APPEARANCE_SECTION)).toEqual({ theme: 'light', contrast: 'more', motion: 'reduce', density: 'compact', pack: 'sd.theme.warm-paper' });
    expect(backend.load()).toMatchObject({ theme: 'light', contrast: 'more' });
  });

  it('follows every change of the section (from any window) and nothing else; stops when asked', async () => {
    const settings = new MemorySettingsBackend();
    const backend = new SettingsAppearanceBackend(settings, copy());
    const heard: unknown[] = [];
    const stop = backend.listen((a) => heard.push(a));
    await flush();
    expect(heard).toEqual([DEFAULT_APPEARANCE]);
    await settings.set(APPEARANCE_SECTION, { motion: 'reduce' });
    await settings.set('sd.general', { repeat: 2 });
    expect(heard.at(-1)).toMatchObject({ motion: 'reduce' });
    expect(heard).toHaveLength(2);
    stop();
    await settings.set(APPEARANCE_SECTION, { motion: 'full' });
    expect(heard).toHaveLength(2);
    expect(backend.load().motion).toBe('reduce');
  });

  it('`events`: follows the changes without asking the engine (a dialog window without settings commands)', async () => {
    const settings = new MemorySettingsBackend();
    const overview = vi.spyOn(settings, 'overview');
    const backend = new SettingsAppearanceBackend(settings, copy(), undefined, 'events');
    const heard: unknown[] = [];
    backend.listen((a) => heard.push(a));
    await flush();
    expect(overview).not.toHaveBeenCalled();
    expect(heard).toEqual([]);
    await settings.set(APPEARANCE_SECTION, { theme: 'dark' });
    expect(heard).toEqual([{ ...DEFAULT_APPEARANCE, theme: 'dark' }]);
  });

  it('failures are reported, never thrown; stopping before the engine answered still unlistens', async () => {
    const settings = new MemorySettingsBackend();
    const failing = Object.assign(Object.create(settings) as SettingsBackend, {
      set: () => Promise.reject(new Error('engine gone')),
      listen: () => Promise.reject(new Error('no events')),
    });
    const warnings: string[] = [];
    const backend = new SettingsAppearanceBackend(failing, copy(), undefined, 'overview', (m) => warnings.push(m));
    backend.save({ ...DEFAULT_APPEARANCE, contrast: 'more' });
    backend.listen(() => undefined);
    await flush();
    expect(warnings).toEqual([
      '[paddock-settings] the appearance could not be saved: engine gone',
      '[paddock-settings] the appearance settings could not be read: no events',
    ]);
    let unlistened = 0;
    const slow = Object.assign(Object.create(settings) as SettingsBackend, { listen: () => Promise.resolve(() => void unlistened++) });
    new SettingsAppearanceBackend(slow, copy()).listen(() => undefined)();
    await flush();
    expect(unlistened).toBe(1);
    const withDefaults = new SettingsAppearanceBackend(settings);
    expect(withDefaults.load()).toEqual(DEFAULT_APPEARANCE);
  });
});

describe('two windows on one engine (rule 40: one writer, one source)', () => {
  /** A window: its own injector with the appearance store over the shared engine, and its own <html>. */
  function windowOn(settings: SettingsBackend) {
    const root = document.createElement('html');
    const injector = createEnvironmentInjector(
      [{ provide: SettingsBackend, useValue: settings }, provideSettingsAppearance({ copy }), provideAppearance({ accent: 'pitwall', root: () => root })],
      TestBed.inject(EnvironmentInjector),
    );
    return { root, store: injector.get(AppearanceStore), destroy: () => injector.destroy() };
  }

  it('a Settings change and a View-menu change in another window never revert each other', async () => {
    const settings = new MemorySettingsBackend();
    const a = windowOn(settings);
    const b = windowOn(settings);
    await flush();
    // Window A: View > Appearance > High contrast. The Settings window (B's side): theme Light, through the form.
    a.store.setContrast('more');
    await settings.set(APPEARANCE_SECTION, { theme: 'light' });
    await flush();
    TestBed.tick();
    for (const w of [a, b]) {
      expect(w.store.appearance()).toMatchObject({ theme: 'light', contrast: 'more' });
      expect(w.root.getAttribute('data-theme')).toBe('light');
      expect(w.root.getAttribute('data-contrast')).toBe('more');
    }
    // B changes motion from its menu; A's earlier contrast and the Settings theme stay.
    b.store.setMotion('reduce');
    await flush();
    TestBed.tick();
    expect(settings.values(APPEARANCE_SECTION)).toMatchObject({ theme: 'light', contrast: 'more', motion: 'reduce' });
    expect(a.store.appearance()).toMatchObject({ theme: 'light', contrast: 'more', motion: 'reduce' });
    a.destroy();
    b.destroy();
  });
});

describe('settingsAppearanceBackend', () => {
  it('asks the engine nothing before the gate opens', async () => {
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
        provideAppearance({ accent: 'pitwall', root: () => document.createElement('html'), backend: settingsAppearanceBackend({ copy, gate: () => ready }) }),
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
    expect(store.theme()).toBe('light');
  });

  it('takes a given settings backend, and keeps its copy in localStorage by default', async () => {
    const settings = new MemorySettingsBackend();
    await settings.set(APPEARANCE_SECTION, { theme: 'dark' });
    TestBed.configureTestingModule({ providers: [provideAppearance({ accent: 'pitwall', root: () => document.createElement('html'), backend: settingsAppearanceBackend({ settings: () => settings }) })] });
    const store = TestBed.inject(AppearanceStore);
    await flush();
    expect(store.theme()).toBe('dark');
    expect(JSON.parse(localStorage.getItem('paddock.appearance') ?? '{}')).toMatchObject({ theme: 'dark' });
    localStorage.clear();
  });
});
