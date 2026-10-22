import { TestBed } from '@angular/core/testing';
import { ThemeService } from './theme.service';

describe('ThemeService', () => {
  beforeEach(() => {
    localStorage.clear();
    delete document.documentElement.dataset['theme'];
  });

  it('follows the system until a theme is chosen, and marks the accent', () => {
    const t = TestBed.inject(ThemeService);
    expect(t.preference()).toBe('system');
    expect(t.scheme()).toBe('light');
    expect(document.documentElement.dataset['accent']).toBe('spotter');
    expect(document.documentElement.dataset['theme']).toBeUndefined();
  });

  it('applies and remembers an explicit choice', () => {
    const t = TestBed.inject(ThemeService);
    t.setPreference('dark');
    expect(t.scheme()).toBe('dark');
    expect(document.documentElement.dataset['theme']).toBe('dark');
    expect(localStorage.getItem('spotter.theme')).toBe('dark');
    t.setPreference('dark');
    expect(t.chartTokens().scheme).toBeDefined();
  });

  it('reads a remembered choice and ignores junk', () => {
    localStorage.setItem('spotter.theme', 'light');
    expect(TestBed.inject(ThemeService).preference()).toBe('light');
    TestBed.resetTestingModule();
    localStorage.setItem('spotter.theme', 'neon');
    expect(TestBed.inject(ThemeService).preference()).toBe('system');
  });

  it('follows the OS scheme changes', () => {
    let listener: ((e: { matches: boolean }) => void) | undefined;
    const original = window.matchMedia;
    window.matchMedia = (() => ({
      matches: false,
      addEventListener: (_: string, l: (e: { matches: boolean }) => void) => (listener = l),
    })) as unknown as typeof window.matchMedia;
    const t = TestBed.inject(ThemeService);
    listener?.({ matches: true });
    expect(t.scheme()).toBe('dark');
    window.matchMedia = original;
  });
});
