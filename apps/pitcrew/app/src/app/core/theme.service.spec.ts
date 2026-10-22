import { TestBed } from '@angular/core/testing';
import { ThemeService } from './theme.service';

function mockMedia(matches: boolean) {
  let handler: ((e: { matches: boolean }) => void) | null = null;
  const mql = {
    matches,
    addEventListener: (_: string, h: (e: { matches: boolean }) => void) => (handler = h),
  };
  vi.spyOn(window, 'matchMedia').mockReturnValue(mql as unknown as MediaQueryList);
  return { fire: (m: boolean) => handler?.({ matches: m }) };
}

describe('ThemeService', () => {
  beforeEach(() => {
    localStorage.clear();
    delete document.documentElement.dataset['theme'];
    TestBed.resetTestingModule();
  });
  afterEach(() => vi.restoreAllMocks());

  it('follows the system by default and reacts to changes', () => {
    const media = mockMedia(false);
    const theme = TestBed.inject(ThemeService);
    expect(theme.preference()).toBe('system');
    expect(theme.theme()).toBe('light');
    expect(document.documentElement.dataset['theme']).toBeUndefined();
    media.fire(true);
    expect(theme.theme()).toBe('dark');
  });

  it('starts dark when the system is dark', () => {
    mockMedia(true);
    expect(TestBed.inject(ThemeService).theme()).toBe('dark');
  });

  it('applies and remembers an explicit choice', () => {
    mockMedia(false);
    const theme = TestBed.inject(ThemeService);
    theme.setPreference('dark');
    expect(theme.theme()).toBe('dark');
    expect(document.documentElement.dataset['theme']).toBe('dark');
    expect(localStorage.getItem('pitcrew.theme')).toBe('dark');
    theme.setPreference('system');
    expect(document.documentElement.dataset['theme']).toBeUndefined();
  });

  it('restores a stored preference and ignores junk', () => {
    mockMedia(false);
    localStorage.setItem('pitcrew.theme', 'dark');
    expect(TestBed.inject(ThemeService).theme()).toBe('dark');
    TestBed.resetTestingModule();
    localStorage.setItem('pitcrew.theme', 'purple');
    expect(TestBed.inject(ThemeService).preference()).toBe('system');
  });

  it('exposes chart tokens that follow the theme, using CSS variables when present', () => {
    mockMedia(false);
    const theme = TestBed.inject(ThemeService);
    const light = theme.chartTokens();
    expect(light.theme).toBe('light');
    expect(light.series).toHaveLength(4);
    theme.setPreference('dark');
    const dark = theme.chartTokens();
    expect(dark.theme).toBe('dark');
    expect(dark.text).not.toBe(light.text);
    document.documentElement.style.setProperty('--accent', '#123456');
    theme.setPreference('light');
    expect(theme.chartTokens().accent).toBe('#123456');
    document.documentElement.style.removeProperty('--accent');
  });
});
