import { describe, expect, it, vi } from 'vitest';
import { ICON_GRID, ICON_STROKE, outlineIcon } from './outline';
import { PD_ICON_PATHS } from './paths';
import { PD_ICON_NAMESPACE, pdIconLiterals, pdIconSvg, registerPaddockIcons } from './register';

/** Every number in path data, to check the glyphs stay on the grid. */
function numbers(d: string): number[] {
  return (d.match(/-?\d*\.?\d+/g) ?? []).map(Number);
}

describe('the pd: icon set', () => {
  const names = Object.keys(PD_ICON_PATHS);

  it('has the glyphs every app redraws', () => {
    for (const n of ['refresh', 'play', 'pause', 'stop', 'trash', 'search', 'copy', 'plus', 'close', 'check', 'more', 'sun', 'moon', 'globe', 'alert', 'info', 'lock', 'shield', 'clock', 'settings', 'download', 'folder', 'edit', 'bell', 'external', 'pin', 'chevron-up', 'chevron-down', 'chevron-left', 'chevron-right']) {
      expect(names, n).toContain(n);
    }
    expect(names.length).toBeGreaterThanOrEqual(30);
  });

  it('draws every glyph on the 24 px grid inside a 2 px margin (absolute coordinates only, arcs checked by end point)', () => {
    for (const [name, d] of Object.entries(PD_ICON_PATHS)) {
      // Only absolute commands are used, so every pair of numbers is a point except arc flags/radii: bound them all by the grid.
      expect(d, name).toMatch(/^[MmLlHhVvCcSsAaZz0-9 .,-]+$/);
      for (const n of numbers(d)) expect(Math.abs(n), `${name} ${n}`).toBeLessThanOrEqual(ICON_GRID);
    }
  });

  it('makes an SVG literal stroked with currentColor, 1.75 wide, with round caps and joins', () => {
    const svg = pdIconSvg('refresh');
    expect(svg).toContain('viewBox="0 0 24 24"');
    expect(svg).toContain('stroke="currentColor"');
    expect(svg).toContain(`stroke-width="${ICON_STROKE}"`);
    expect(svg).toContain('stroke-linecap="round"');
    expect(svg).toContain('stroke-linejoin="round"');
    expect(svg).toContain('fill="none"');
    expect(svg).not.toContain('http://www.w3.org/2000/svg" xmlns'); // single namespace
  });

  it('keeps markup out of a literal: path data cannot close the attribute', () => {
    const svg = outlineIcon('M0 0"/><script>x</script>');
    expect(svg).not.toContain('<script');
    expect(svg.match(/</g)).toHaveLength(3); // <svg>, <path>, </svg>: nothing else
  });

  it('registers every glyph as a literal in the pd namespace, trusted once, without any URL', () => {
    const added: Array<[string, string, unknown]> = [];
    const registry = { addSvgIconLiteralInNamespace: (ns: string, n: string, l: never) => added.push([ns, n, l]) };
    const trust = vi.fn((v: string) => ({ trusted: v }));
    registerPaddockIcons(registry, { bypassSecurityTrustHtml: trust });
    expect(added.map((a) => a[1])).toEqual(names);
    expect(new Set(added.map((a) => a[0]))).toEqual(new Set([PD_ICON_NAMESPACE]));
    expect(trust).toHaveBeenCalledTimes(names.length);
    for (const [literal] of pdIconLiterals().map((l) => [l[1]])) expect(literal).not.toMatch(/https?:\/\/(?!www\.w3\.org)/);
  });
});
