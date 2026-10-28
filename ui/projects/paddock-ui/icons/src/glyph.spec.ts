import { provideZonelessChangeDetection } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { MatIconRegistry } from '@angular/material/icon';
import { firstValueFrom } from 'rxjs';
import { describe, expect, it, vi } from 'vitest';
import { PdGlyph } from './glyph';
import { PD_ICON_DOTS, PD_ICON_PATHS, type PdIconName } from './paths';
import { isPdIcon, pdIconId, providePaddockIcons } from './register';

const NAMES = Object.keys(PD_ICON_PATHS) as PdIconName[];

/** The geometry of an svg: its path data, dots (cx, cy, r) and the presentation attributes the family fixes. */
function geometry(svg: Element): { d: string[]; dots: number[][]; viewBox: string | null; stroke: string | null; width: string | null; fill: string | null; cap: string | null } {
  return {
    d: [...svg.querySelectorAll('path')].map((p) => p.getAttribute('d') ?? ''),
    dots: [...svg.querySelectorAll('circle')].map((c) => ['cx', 'cy', 'r'].map((a) => Number(c.getAttribute(a)))),
    viewBox: svg.getAttribute('viewBox'),
    stroke: svg.getAttribute('stroke'),
    width: svg.getAttribute('stroke-width'),
    fill: svg.getAttribute('fill'),
    cap: svg.getAttribute('stroke-linecap'),
  };
}

describe('the pd: set in MatIconRegistry', () => {
  it('resolves every name through the registry with the geometry of the path constants (same viewBox, stroke and solid dots)', async () => {
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection(), providePaddockIcons()] });
    TestBed.inject(MatIconRegistry); // the initializer runs with the first injection of the app
    const registry = TestBed.inject(MatIconRegistry);
    for (const name of NAMES) {
      const svg = await firstValueFrom(registry.getNamedSvgIcon(name, 'pd'));
      const g = geometry(svg);
      expect(g.viewBox, name).toBe('0 0 24 24');
      expect(g.stroke, name).toBe('currentColor');
      expect(g.width, name).toBe('1.5');
      expect(g.fill, name).toBe('none');
      expect(g.cap, name).toBe('round');
      expect(g.d, name).toEqual(PD_ICON_PATHS[name] === '' ? [] : [PD_ICON_PATHS[name]]);
      expect(g.dots, name).toEqual((PD_ICON_DOTS[name] ?? []).map((d) => [...d]));
      for (const c of svg.querySelectorAll('circle')) {
        expect(c.getAttribute('fill'), name).toBe('currentColor');
        expect(c.getAttribute('stroke'), name).toBe('none');
      }
    }
  });

  it('names a glyph with or without the namespace, and knows what is not one', () => {
    expect(pdIconId('refresh')).toBe('pd:refresh');
    expect(pdIconId('pd:refresh')).toBe('pd:refresh');
    expect(isPdIcon('pd:more')).toBe(true);
    expect(isPdIcon('more')).toBe(true);
    for (const n of ['', 'pd:', 'pw:models', 'toString', 'constructor', '__proto__']) expect(isPdIcon(n), n).toBe(false);
  });
});

describe('pd-glyph', () => {
  it('draws <mat-icon svgIcon="pd:..."> at its size, decorative, with no provider needed', async () => {
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection()] });
    const fixture = TestBed.createComponent(PdGlyph);
    fixture.componentRef.setInput('name', 'pd:alert');
    fixture.componentRef.setInput('size', 20);
    await fixture.whenStable();
    const el: HTMLElement = fixture.nativeElement;
    expect(el.getAttribute('aria-hidden')).toBe('true');
    const icon = el.querySelector('mat-icon');
    expect(icon?.getAttribute('data-mat-icon-name')).toBe('alert');
    expect(icon?.getAttribute('data-mat-icon-namespace')).toBe('pd');
    expect((icon as HTMLElement).style.width).toBe('20px');
    const g = geometry(el.querySelector('svg') as Element);
    expect(g.d).toEqual([PD_ICON_PATHS.alert]);
    // 20 px: the authored radius is above the floor (0.75 * 24 / 20 = 0.9), so it is kept.
    expect(g.dots).toEqual((PD_ICON_DOTS.alert ?? []).map((d) => [...d]));
  });

  it('authors every dot at least as heavy as the stroke at 16 px (radius >= 1.125, in the data), and draws them the same everywhere', async () => {
    const authored: Record<string, number[]> = { alert: [1.125], info: [1.125], keyboard: [1.125, 1.125, 1.125, 1.125], more: [1.5, 1.5, 1.5] };
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection()] });
    const registry = TestBed.inject(MatIconRegistry);
    for (const [name, radii] of Object.entries(authored)) {
      expect((PD_ICON_DOTS[name as PdIconName] ?? []).map((d) => d[2]), name).toEqual(radii);
      const fixture = TestBed.createComponent(PdGlyph);
      fixture.componentRef.setInput('name', name);
      await fixture.whenStable();
      const glyph = [...(fixture.nativeElement as HTMLElement).querySelectorAll('circle')].map((c) => Number(c.getAttribute('r')));
      const bare = [...(await firstValueFrom(registry.getNamedSvgIcon(name, 'pd'))).querySelectorAll('circle')].map((c) => Number(c.getAttribute('r')));
      expect(glyph, name).toEqual(radii);
      expect(bare, name).toEqual(radii);
    }
  });

  it('warns in dev mode when the glyph had to register the set itself (no provider), once', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection()] });
    TestBed.createComponent(PdGlyph);
    TestBed.createComponent(PdGlyph);
    expect(warn).toHaveBeenCalledTimes(1);
    expect(String(warn.mock.calls[0]?.[0])).toContain('providePaddockIcons');
    warn.mockRestore();
  });

  it('does not warn when the provider registered the set', () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection(), providePaddockIcons()] });
    TestBed.inject(MatIconRegistry);
    TestBed.createComponent(PdGlyph);
    expect(warn).not.toHaveBeenCalled();
    warn.mockRestore();
  });

  it('draws an empty box, not an error, for a name that is not in the set', async () => {
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection()] });
    const fixture = TestBed.createComponent(PdGlyph);
    fixture.componentRef.setInput('name', 'nope');
    await fixture.whenStable();
    expect(fixture.nativeElement.querySelector('mat-icon')).toBeNull();
    expect(fixture.nativeElement.querySelector('svg')).toBeNull();
  });
});
