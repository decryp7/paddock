import { TestBed } from '@angular/core/testing';
import { AppearanceStore } from '@decryp7/paddock-ui/shell';
import { signal } from '@angular/core';
import { ThemeService } from './theme.service';

describe('ThemeService (chart tokens over the shell appearance)', () => {
  const scheme = signal<'light' | 'dark'>('light');
  beforeEach(() => {
    scheme.set('light');
    TestBed.resetTestingModule();
    TestBed.configureTestingModule({ providers: [{ provide: AppearanceStore, useValue: { resolved: () => ({ scheme: scheme(), highContrast: false, reducedMotion: false }) } }] });
  });

  it('exposes chart tokens that follow the shell theme, using CSS variables when present', () => {
    const theme = TestBed.inject(ThemeService);
    const light = theme.chartTokens();
    expect(light.theme).toBe('light');
    expect(light.series).toHaveLength(4);
    scheme.set('dark');
    const dark = theme.chartTokens();
    expect(dark.theme).toBe('dark');
    expect(dark.text).not.toBe(light.text);
  });

  it('re-reads the tokens after the page applied a new look', async () => {
    const theme = TestBed.inject(ThemeService);
    expect(theme.chartTokens().accent).not.toBe('#123456');
    document.documentElement.style.setProperty('--accent', '#123456');
    document.documentElement.dataset['accent'] = 'pitcrew';
    await new Promise((r) => setTimeout(r));
    expect(theme.chartTokens().accent).toBe('#123456');
    document.documentElement.style.removeProperty('--accent');
    delete document.documentElement.dataset['accent'];
  });
});
