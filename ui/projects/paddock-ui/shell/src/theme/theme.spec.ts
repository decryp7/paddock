import { ApplicationRef, Injector, runInInjectionContext } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { applyStoredAppearance, AppearanceStore, BrowserAppearanceBackend, IpcAppearanceBackend, MATCH_MEDIA, provideAppearance, provideThemes, type MatchMedia } from '@decryp7/paddock-ui/shell';
import { ContributionRegistry } from '@decryp7/paddock-ui/shell/contributions';
import { msg } from '@decryp7/paddock-ui/shell/contributions';
import { DEFAULT_APPEARANCE, parseAppearance, resolveAppearance, sameAppearance, serializeAppearance, type Appearance } from './appearance';
import { APPEARANCE_EVENT, APPEARANCE_KEY, type AppearanceChannel, type AppearanceStorage } from './appearance-backend';
import { applyAppearanceTo } from './appearance.store';
import { contrast, parseColor, tryParseColor } from './color';
import { defineTheme, THEMES, themeProblems, themeTokensFor, type ThemeDef } from './theme';

class MemoryStorage implements AppearanceStorage {
  readonly map = new Map<string, string>();
  failWrites = false;
  getItem(key: string): string | null {
    return this.map.get(key) ?? null;
  }
  setItem(key: string, value: string): void {
    if (this.failWrites) throw new Error('quota');
    this.map.set(key, value);
  }
}

/** Every channel opened on one bus hears the others' messages (like BroadcastChannel, never its own). */
class Bus {
  private readonly members = new Set<FakeChannel>();
  open(): FakeChannel {
    const c = new FakeChannel(this);
    this.members.add(c);
    return c;
  }
  deliver(from: FakeChannel, data: unknown): void {
    for (const m of this.members) if (m !== from) m.receive(data);
  }
}

class FakeChannel implements AppearanceChannel {
  private readonly listeners = new Set<(e: MessageEvent) => void>();
  constructor(private readonly bus: Bus) {}
  postMessage(message: unknown): void {
    this.bus.deliver(this, structuredClone(message));
  }
  addEventListener(_: 'message', l: (e: MessageEvent) => void): void {
    this.listeners.add(l);
  }
  removeEventListener(_: 'message', l: (e: MessageEvent) => void): void {
    this.listeners.delete(l);
  }
  receive(data: unknown): void {
    for (const l of this.listeners) l({ data } as MessageEvent);
  }
  get listening(): number {
    return this.listeners.size;
  }
}

/** A matchMedia whose answers the test sets and whose change listeners it fires. */
function fakeMedia(initial: Partial<Record<string, boolean>> = {}): { mm: MatchMedia; set(query: string, matches: boolean): void } {
  const state = new Map(Object.entries(initial).map(([k, v]) => [k, !!v]));
  const listeners = new Map<string, Set<() => void>>();
  return {
    mm: (q) => ({
      get matches() {
        return state.get(q) ?? false;
      },
      addEventListener: (_: string, l: () => void) => void (listeners.get(q) ?? listeners.set(q, new Set()).get(q))?.add(l),
      removeEventListener: (_: string, l: () => void) => void listeners.get(q)?.delete(l),
    }) as unknown as ReturnType<MatchMedia>,
    set(q, matches) {
      state.set(q, matches);
      for (const l of listeners.get(q) ?? []) l();
    },
  };
}

const CONSOLE = defineTheme({
  id: 'sd.theme.console',
  name: msg('sd.theme.console', 'Console'),
  tokens: { common: { '--pd-radius-card': '2px' }, dark: { '--pd-bg': '#000000', '--pd-text': '#e6ffe6' }, light: { '--pd-bg': '#fafff9' } },
});

describe('appearance (pure)', () => {
  it('parses a stored value and falls back field by field', () => {
    expect(parseAppearance({ version: 1, theme: 'light', contrast: 'more', motion: 'reduce', density: 'compact', pack: 'db.theme.console' })).toEqual({
      theme: 'light',
      contrast: 'more',
      motion: 'reduce',
      density: 'compact',
      pack: 'db.theme.console',
    });
    expect(parseAppearance({ version: 1, theme: 'neon', contrast: 3, pack: 'Not An Id' })).toEqual(DEFAULT_APPEARANCE);
  });

  it('a wrong version, a non-object or null is the default', () => {
    for (const raw of [null, undefined, 'dark', 42, [], { version: 2, theme: 'dark' }]) expect(parseAppearance(raw)).toEqual(DEFAULT_APPEARANCE);
  });

  it('round-trips through its stored form', () => {
    const a: Appearance = { theme: 'dark', contrast: 'standard', motion: 'full', density: 'comfortable', pack: null };
    expect(parseAppearance(JSON.parse(JSON.stringify(serializeAppearance(a))))).toEqual(a);
    expect(sameAppearance(a, { ...a })).toBe(true);
    expect(sameAppearance(a, { ...a, pack: 'x.y' })).toBe(false);
  });

  it('resolves system choices against the OS and fixed ones without it', () => {
    const os = { dark: false, moreContrast: true, reducedMotion: true };
    expect(resolveAppearance(DEFAULT_APPEARANCE, os)).toEqual({ scheme: 'light', highContrast: true, reducedMotion: true });
    expect(resolveAppearance({ ...DEFAULT_APPEARANCE, theme: 'dark', contrast: 'standard', motion: 'full' }, os)).toEqual({
      scheme: 'dark',
      highContrast: false,
      reducedMotion: false,
    });
    expect(resolveAppearance({ ...DEFAULT_APPEARANCE, contrast: 'more', motion: 'reduce' }, { dark: true, moreContrast: false, reducedMotion: false })).toEqual({
      scheme: 'dark',
      highContrast: true,
      reducedMotion: true,
    });
  });
});

describe('theme packs', () => {
  it('a valid pack has no problems and is a contribution kind', () => {
    expect(themeProblems(CONSOLE)).toEqual([]);
    expect(THEMES.identity(CONSOLE)).toBe('sd.theme.console');
    expect(THEMES.check?.(CONSOLE, { has: () => false, items: () => [] })).toEqual([]);
  });

  it('reports a bad id, a missing name and missing tokens', () => {
    expect(themeProblems({ id: 'Console', name: 'Console' })).toEqual([
      'id "Console" is not a namespaced id (db.theme.console)',
      "name has no message id (use msg('<ns>.<name>', 'Text'))",
      'has no tokens',
    ]);
    expect(themeProblems(null)).toEqual(['is not a theme definition']);
  });

  it('refuses token names that are not --pd-* and values that could escape a custom property', () => {
    const p = themeProblems(
      defineTheme({
        id: 'sd.theme.bad',
        name: msg('sd.theme.bad', 'Bad'),
        tokens: { common: { color: 'red', '--pd-x': 'url(https://example.com/a.png)', '--pd-y': 'red; background: blue', '--pd-z': ' ' }, dark: 'x' as never },
      }),
    );
    expect(p).toContain('tokens.common names "color", which is not a --pd-* token');
    expect(p.some((s) => s.startsWith('tokens.common.--pd-x has a value'))).toBe(true);
    expect(p.some((s) => s.startsWith('tokens.common.--pd-y has a value'))).toBe(true);
    expect(p).toContain('tokens.common.--pd-z is empty');
    expect(p).toContain('tokens.dark is not a map of custom properties');
  });

  it('enforces the limits (token count and value length)', () => {
    const many = Object.fromEntries(Array.from({ length: 201 }, (_, i) => [`--pd-t-${i}`, '1px']));
    const p = themeProblems({ id: 'sd.theme.big', name: msg('sd.theme.big', 'Big'), tokens: { common: { ...many, '--pd-long': 'x'.repeat(201) } } });
    expect(p).toContain('tokens.common has 202 tokens (at most 200)');
    expect(p).toContain('tokens.common.--pd-long is longer than 200 characters');
  });

  it('measures the pairs a pack sets by the token targets (text AA, boundaries 3:1)', () => {
    const p = themeProblems(
      defineTheme({
        id: 'sd.theme.grey',
        name: msg('sd.theme.grey', 'Grey'),
        tokens: { common: { '--pd-surface-1': '#777777' }, dark: { '--pd-text': '#888888' }, light: { '--pd-border-control': '#7a7a7a', '--pd-text': '#000000' } },
      }),
    );
    expect(p.filter((s) => s.startsWith('dark: --pd-text on --pd-surface-1'))).toHaveLength(1);
    expect(p.some((s) => s.startsWith('light: --pd-border-control on --pd-surface-1'))).toBe(true);
    expect(p.some((s) => s.startsWith('light: --pd-text on'))).toBe(false);
  });

  it('a pack sets common tokens then its scheme', () => {
    expect(themeTokensFor(CONSOLE, 'dark')).toEqual({ '--pd-radius-card': '2px', '--pd-bg': '#000000', '--pd-text': '#e6ffe6' });
    expect(themeTokensFor(CONSOLE, 'light')).toEqual({ '--pd-radius-card': '2px', '--pd-bg': '#fafff9' });
  });
});

describe('colour maths', () => {
  it('parses hex and rgb() and refuses anything else without throwing', () => {
    expect(parseColor('#fff')).toEqual({ r: 255, g: 255, b: 255, a: 1 });
    expect(tryParseColor('rgb(0 0 0 / 0.5)')).toEqual({ r: 0, g: 0, b: 0, a: 0.5 });
    expect(tryParseColor('hotpink')).toBeNull();
    expect(tryParseColor('rgb(300 0 0)')).toBeNull();
    expect(contrast(parseColor('#000'), parseColor('#fff'))).toBeCloseTo(21, 5);
  });
});

describe('applyAppearanceTo', () => {
  it('sets the attributes and the pack tokens, and removes exactly what the last pack set', () => {
    const root = document.createElement('html');
    const resolved = { scheme: 'dark', highContrast: false, reducedMotion: false } as const;
    applyAppearanceTo(root, { ...DEFAULT_APPEARANCE, theme: 'dark', contrast: 'more', motion: 'reduce', pack: CONSOLE.id }, 'pacenotes', resolved, CONSOLE);
    expect(root.getAttribute('data-theme')).toBe('dark');
    expect(root.getAttribute('data-accent')).toBe('pacenotes');
    expect(root.getAttribute('data-contrast')).toBe('more');
    expect(root.getAttribute('data-motion')).toBe('reduce');
    expect(root.getAttribute('data-pack')).toBe('sd.theme.console');
    expect(root.style.getPropertyValue('--pd-bg')).toBe('#000000');
    root.style.setProperty('--pd-own', '1px');
    applyAppearanceTo(root, { ...DEFAULT_APPEARANCE, theme: 'light' }, 'pacenotes', { ...resolved, scheme: 'light' }, CONSOLE);
    expect(root.style.getPropertyValue('--pd-bg')).toBe('#fafff9');
    expect(root.style.getPropertyValue('--pd-text')).toBe('');
    applyAppearanceTo(root, DEFAULT_APPEARANCE, 'pacenotes', resolved, null);
    expect(root.style.getPropertyValue('--pd-bg')).toBe('');
    expect(root.style.getPropertyValue('--pd-radius-card')).toBe('');
    expect(root.style.getPropertyValue('--pd-own')).toBe('1px');
    expect(root.hasAttribute('data-pack')).toBe(false);
    expect(root.hasAttribute('data-theme')).toBe(false);
  });

  it('applies no pack in high contrast, so a pack cannot weaken it', () => {
    const root = document.createElement('html');
    applyAppearanceTo(root, { ...DEFAULT_APPEARANCE, contrast: 'more' }, 'debrief', { scheme: 'dark', highContrast: true, reducedMotion: false }, CONSOLE);
    expect(root.style.getPropertyValue('--pd-bg')).toBe('');
    expect(root.hasAttribute('data-pack')).toBe(false);
  });
});

describe('BrowserAppearanceBackend', () => {
  it('keeps the choice and tells the other windows of the origin, never itself', () => {
    const storage = new MemoryStorage();
    const bus = new Bus();
    const a = new BrowserAppearanceBackend({ storage, channel: bus.open() });
    const b = new BrowserAppearanceBackend({ storage, channel: bus.open() });
    const heardByA: Appearance[] = [];
    const heardByB: Appearance[] = [];
    a.listen((x) => heardByA.push(x));
    const stop = b.listen((x) => heardByB.push(x));
    const dark = { ...DEFAULT_APPEARANCE, theme: 'dark' } as const;
    a.save(dark);
    expect(heardByB).toEqual([dark]);
    expect(heardByA).toEqual([]);
    expect(b.load()).toEqual(dark);
    expect(JSON.parse(storage.getItem(APPEARANCE_KEY) ?? '')).toMatchObject({ version: 1, theme: 'dark' });
    stop();
    a.save(DEFAULT_APPEARANCE);
    expect(heardByB).toHaveLength(1);
  });

  it('a corrupt stored value or a failing storage is the default, never an exception', () => {
    const storage = new MemoryStorage();
    storage.map.set(APPEARANCE_KEY, '{not json');
    const backend = new BrowserAppearanceBackend({ storage, channel: null });
    expect(backend.load()).toEqual(DEFAULT_APPEARANCE);
    storage.failWrites = true;
    expect(() => backend.save({ ...DEFAULT_APPEARANCE, theme: 'light' })).not.toThrow();
    expect(backend.listen(() => undefined)).toBeTypeOf('function');
    expect(new BrowserAppearanceBackend({ storage: null, channel: null }).load()).toEqual(DEFAULT_APPEARANCE);
  });

  it('a malformed broadcast is read as the default appearance', () => {
    const bus = new Bus();
    const a = bus.open();
    const b = new BrowserAppearanceBackend({ storage: new MemoryStorage(), channel: bus.open() });
    const heard: Appearance[] = [];
    b.listen((x) => heard.push(x));
    a.postMessage({ version: 99, theme: 'light' });
    expect(heard).toEqual([DEFAULT_APPEARANCE]);
  });
});

describe('IpcAppearanceBackend', () => {
  it('emits paddock://appearance on save and parses what it hears', async () => {
    const emitted: [string, unknown][] = [];
    let handler: ((p: unknown) => void) | null = null;
    let unlistened = 0;
    const ipc = {
      emit: (e: string, p: unknown) => (emitted.push([e, p]), Promise.resolve()),
      listen: (_: string, h: (p: unknown) => void) => ((handler = h), Promise.resolve(() => void unlistened++)),
    };
    const backend = new IpcAppearanceBackend(ipc, new MemoryStorage());
    const heard: Appearance[] = [];
    const stop = backend.listen((a) => heard.push(a));
    await Promise.resolve();
    backend.save({ ...DEFAULT_APPEARANCE, contrast: 'more' });
    expect(emitted).toEqual([[APPEARANCE_EVENT, expect.objectContaining({ version: 1, contrast: 'more' })]]);
    expect(backend.load().contrast).toBe('more');
    (handler as unknown as (p: unknown) => void)({ version: 1, theme: 'light' });
    expect(heard).toEqual([{ ...DEFAULT_APPEARANCE, theme: 'light' }]);
    stop();
    expect(unlistened).toBe(1);
  });

  it('stopping before the listener is ready still unlistens; failures of the bus are swallowed', async () => {
    let unlistened = 0;
    const ipc = { emit: () => Promise.reject(new Error('no bus')), listen: () => Promise.resolve(() => void unlistened++) };
    const backend = new IpcAppearanceBackend(ipc, null);
    backend.listen(() => undefined)();
    await Promise.resolve();
    await Promise.resolve();
    expect(unlistened).toBe(1);
    expect(() => backend.save(DEFAULT_APPEARANCE)).not.toThrow();
    const failing = new IpcAppearanceBackend({ emit: () => Promise.resolve(), listen: () => Promise.reject(new Error('x')) }, null);
    expect(() => failing.listen(() => undefined)()).not.toThrow();
  });
});

describe('AppearanceStore', () => {
  function setup(opts: { storage?: MemoryStorage; bus?: Bus; media?: ReturnType<typeof fakeMedia>; themes?: ThemeDef[] } = {}) {
    const root = document.createElement('html');
    const storage = opts.storage ?? new MemoryStorage();
    const bus = opts.bus ?? new Bus();
    const media = opts.media ?? fakeMedia();
    TestBed.resetTestingModule();
    TestBed.configureTestingModule({
      providers: [
        provideAppearance({ accent: 'sighter', root: () => root, backend: () => new BrowserAppearanceBackend({ storage, channel: bus.open() }) }),
        provideThemes(...(opts.themes ?? [CONSOLE])),
        { provide: MATCH_MEDIA, useValue: media.mm },
      ],
    });
    const store = TestBed.inject(AppearanceStore);
    TestBed.tick();
    return { root, store, storage, bus, media };
  }

  it('applies the stored choice at start and follows the OS while on system', () => {
    const storage = new MemoryStorage();
    storage.setItem(APPEARANCE_KEY, JSON.stringify(serializeAppearance({ ...DEFAULT_APPEARANCE, density: 'compact' })));
    const { root, store, media } = setup({ storage, media: fakeMedia({ '(prefers-color-scheme: light)': true }) });
    expect(root.getAttribute('data-accent')).toBe('sighter');
    expect(root.getAttribute('data-density')).toBe('compact');
    expect(store.resolved().scheme).toBe('light');
    media.set('(prefers-color-scheme: light)', false);
    media.set('(prefers-reduced-motion: reduce)', true);
    expect(store.resolved()).toEqual({ scheme: 'dark', highContrast: false, reducedMotion: true });
  });

  it('a change is applied, saved and announced; the same choice twice does nothing', () => {
    const { root, store, storage } = setup();
    store.setTheme('light');
    store.setContrast('more');
    store.setMotion('reduce');
    store.setDensity('comfortable');
    TestBed.tick();
    expect(root.getAttribute('data-theme')).toBe('light');
    expect(root.getAttribute('data-contrast')).toBe('more');
    expect(root.getAttribute('data-motion')).toBe('reduce');
    expect(root.getAttribute('data-density')).toBe('comfortable');
    expect(parseAppearance(JSON.parse(storage.getItem(APPEARANCE_KEY) ?? ''))).toEqual(store.appearance());
    storage.map.clear();
    store.setTheme('light');
    expect(storage.map.size).toBe(0);
  });

  it('two windows: a theme change in one is shown in the other at once', () => {
    const bus = new Bus();
    const storage = new MemoryStorage();
    const other = new BrowserAppearanceBackend({ storage, channel: bus.open() });
    const { root, store } = setup({ bus, storage });
    other.save({ ...DEFAULT_APPEARANCE, theme: 'dark', pack: CONSOLE.id });
    TestBed.tick();
    expect(store.theme()).toBe('dark');
    expect(store.pack()?.id).toBe(CONSOLE.id);
    expect(root.getAttribute('data-theme')).toBe('dark');
    expect(root.style.getPropertyValue('--pd-bg')).toBe('#000000');
  });

  it('a pack that is not registered in this window is no pack; the registry lists the packs', () => {
    const { store } = setup({ themes: [] });
    store.setPack('db.theme.missing');
    expect(store.pack()).toBeNull();
    expect(store.packs()).toEqual([]);
    expect(TestBed.inject(ContributionRegistry).kinds.some((k) => k.name === 'themes')).toBe(true);
  });

  it('stops listening when the window goes away', () => {
    const bus = new Bus();
    const channel = bus.open();
    TestBed.resetTestingModule();
    TestBed.configureTestingModule({
      providers: [provideAppearance({ accent: 'debrief', root: () => document.createElement('html'), backend: () => new BrowserAppearanceBackend({ storage: null, channel }) })],
    });
    TestBed.inject(ApplicationRef);
    expect((channel as FakeChannel).listening).toBe(1);
    TestBed.resetTestingModule();
    expect((channel as FakeChannel).listening).toBe(0);
  });
});

describe('defaults', () => {
  afterEach(() => localStorage.clear());

  it('provideAppearance without a backend or root uses localStorage, a BroadcastChannel and <html>', () => {
    TestBed.resetTestingModule();
    TestBed.configureTestingModule({ providers: [provideAppearance({ accent: 'spotter' })] });
    const store = TestBed.inject(AppearanceStore);
    TestBed.tick();
    expect(document.documentElement.getAttribute('data-accent')).toBe('spotter');
    store.setDensity('compact');
    expect(JSON.parse(localStorage.getItem(APPEARANCE_KEY) ?? '{}')).toMatchObject({ density: 'compact' });
    expect(store.contrast()).toBe('system');
    expect(store.motion()).toBe('system');
    expect(store.density()).toBe('compact');
    TestBed.resetTestingModule();
    for (const a of ['data-accent', 'data-density']) document.documentElement.removeAttribute(a);
  });

  it('without matchMedia the OS is taken as dark, standard contrast and full motion', () => {
    TestBed.resetTestingModule();
    TestBed.configureTestingModule({
      providers: [
        provideAppearance({ accent: 'debrief', root: () => document.createElement('html'), backend: () => new BrowserAppearanceBackend({ storage: null, channel: null }) }),
        { provide: MATCH_MEDIA, useValue: null },
      ],
    });
    expect(TestBed.inject(AppearanceStore).resolved()).toEqual({ scheme: 'dark', highContrast: false, reducedMotion: false });
  });

  it('a browser without BroadcastChannel keeps the choice but has no cross-window sync', () => {
    const original = globalThis.BroadcastChannel;
    (globalThis as { BroadcastChannel?: unknown }).BroadcastChannel = undefined;
    try {
      const backend = new BrowserAppearanceBackend({ storage: new MemoryStorage() });
      backend.save({ ...DEFAULT_APPEARANCE, theme: 'light' });
      expect(backend.load().theme).toBe('light');
      expect(backend.listen(() => undefined)).toBeTypeOf('function');
    } finally {
      globalThis.BroadcastChannel = original;
    }
  });
});

describe('applyStoredAppearance', () => {
  it('puts the stored scheme, contrast and motion on the root before Angular starts', () => {
    const storage = new MemoryStorage();
    storage.setItem(APPEARANCE_KEY, JSON.stringify(serializeAppearance({ ...DEFAULT_APPEARANCE, theme: 'light', contrast: 'more' })));
    const root = document.createElement('html');
    applyStoredAppearance(root, 'pitwall', new BrowserAppearanceBackend({ storage, channel: null }));
    expect(root.getAttribute('data-theme')).toBe('light');
    expect(root.getAttribute('data-contrast')).toBe('more');
    expect(root.getAttribute('data-accent')).toBe('pitwall');
  });

  it('defaults to the browser backend without a channel', () => {
    const root = document.createElement('html');
    runInInjectionContext(Injector.create({ providers: [] }), () => applyStoredAppearance(root, 'debrief'));
    expect(root.getAttribute('data-accent')).toBe('debrief');
  });
});
