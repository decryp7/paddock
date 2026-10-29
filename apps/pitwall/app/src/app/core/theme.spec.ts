import { TestBed } from '@angular/core/testing';
import { THEME_TOKENS } from '../shared/theme-tokens';
import { ThemeService, isMac } from './theme';

type Listener = (e: { matches: boolean }) => void;

function mockMedia(matches: boolean) {
  let listener: Listener | null = null;
  const mql = { matches, addEventListener: (_: string, l: Listener) => (listener = l) };
  vi.spyOn(window, 'matchMedia').mockReturnValue(mql as unknown as MediaQueryList);
  return { fire: (m: boolean) => listener?.({ matches: m }) };
}

describe('ThemeService', () => {
  beforeEach(() => localStorage.clear());

  afterEach(() => {
    vi.restoreAllMocks();
    localStorage.clear();
    document.documentElement.removeAttribute('data-theme');
    document.documentElement.classList.remove('mac');
  });

  it('follows the system by default and leaves data-theme off', () => {
    mockMedia(true);
    const t = TestBed.inject(ThemeService);
    TestBed.tick();
    expect(t.choice()).toBe('system');
    expect(t.resolved()).toBe('dark');
    expect(document.documentElement.hasAttribute('data-theme')).toBe(false);
    expect(t.tokens()).toBe(THEME_TOKENS.dark);
  });

  it('tracks live changes of the system preference while on system', () => {
    const media = mockMedia(false);
    const t = TestBed.inject(ThemeService);
    expect(t.resolved()).toBe('light');
    media.fire(true);
    expect(t.resolved()).toBe('dark');
    expect(t.tokens()).toBe(THEME_TOKENS.dark);
  });

  it('applies an explicit choice to <html>, remembers it and ignores the system', () => {
    const media = mockMedia(true);
    const t = TestBed.inject(ThemeService);
    t.set('light');
    TestBed.tick();
    expect(t.resolved()).toBe('light');
    expect(document.documentElement.getAttribute('data-theme')).toBe('light');
    expect(localStorage.getItem('pitwall.theme')).toBe('light');
    media.fire(false);
    expect(t.resolved()).toBe('light');
    t.set('system');
    TestBed.tick();
    expect(document.documentElement.hasAttribute('data-theme')).toBe(false);
    expect(localStorage.getItem('pitwall.theme')).toBe('system');
  });

  it('starts from the remembered choice', () => {
    mockMedia(false);
    localStorage.setItem('pitwall.theme', 'dark');
    const t = TestBed.inject(ThemeService);
    expect(t.choice()).toBe('dark');
    expect(t.resolved()).toBe('dark');
  });

  it('falls back to system for an unknown stored value', () => {
    mockMedia(false);
    localStorage.setItem('pitwall.theme', 'purple');
    expect(TestBed.inject(ThemeService).choice()).toBe('system');
  });

  it('survives unavailable storage', () => {
    mockMedia(false);
    vi.spyOn(Storage.prototype, 'getItem').mockImplementation(() => {
      throw new Error('blocked');
    });
    vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => {
      throw new Error('blocked');
    });
    const t = TestBed.inject(ThemeService);
    t.set('dark');
    expect(() => TestBed.tick()).not.toThrow();
    expect(t.choice()).toBe('dark');
    expect(document.documentElement.getAttribute('data-theme')).toBe('dark');
  });

  it('treats a missing matchMedia as light', () => {
    vi.spyOn(window, 'matchMedia').mockReturnValue(undefined as unknown as MediaQueryList);
    expect(TestBed.inject(ThemeService).resolved()).toBe('light');
  });

  it('toggles the platform classes', () => {
    mockMedia(false);
    const t = TestBed.inject(ThemeService);
    const cl = document.documentElement.classList;
    t.applyPlatform({ mac: true });
    expect(cl.contains('mac')).toBe(true);
    t.applyPlatform({ mac: false });
    expect(cl.contains('mac')).toBe(false);
    // The window is opaque: nothing marks it translucent.
    expect(cl.contains('translucent')).toBe(false);
  });
});

describe('isMac', () => {
  it('recognises macOS user agents but not iOS ones', () => {
    expect(isMac('Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605')).toBe(true);
    expect(isMac('Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X)')).toBe(false);
    expect(isMac('Mozilla/5.0 (iPad; CPU OS 17_0 like Mac OS X) Macintosh')).toBe(false);
    expect(isMac('Mozilla/5.0 (Windows NT 10.0; Win64; x64)')).toBe(false);
  });
});
