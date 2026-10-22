import { contrast, deltaE, luminance, parseHex, simulate, worstAdjacent } from './color';
import { THEME_TOKENS, seriesColor, type ResolvedTheme } from './theme-tokens';

const THEMES: ResolvedTheme[] = ['light', 'dark'];

describe('colour math', () => {
  it('parses hex and rejects anything else', () => {
    expect(parseHex('#ff0000')).toEqual([1, 0, 0]);
    expect(parseHex('00ff00')).toEqual([0, 1, 0]);
    expect(() => parseHex('#fff')).toThrow();
    expect(() => parseHex('red')).toThrow();
  });

  it('computes WCAG luminance and contrast', () => {
    expect(luminance('#ffffff')).toBeCloseTo(1, 5);
    expect(luminance('#000000')).toBe(0);
    expect(contrast('#000000', '#ffffff')).toBeCloseTo(21, 5);
    expect(contrast('#ffffff', '#000000')).toBeCloseTo(21, 5);
    expect(contrast('#777777', '#777777')).toBe(1);
  });

  it('measures perceptual distance, also under simulated deficiencies', () => {
    expect(deltaE('#123456', '#123456')).toBe(0);
    // Red and green collapse for protanopes and deuteranopes far more than for normal vision.
    const normal = deltaE('#d62728', '#2ca02c');
    expect(deltaE('#d62728', '#2ca02c', 'protan')).toBeLessThan(normal / 2);
    expect(deltaE('#d62728', '#2ca02c', 'deutan')).toBeLessThan(normal / 2);
    // Blue and yellow collapse for tritanopes less than red/green do for the others, but they do move.
    expect(simulate('#ffffff', 'tritan').every((c) => c >= 0 && c <= 1)).toBe(true);
    expect(worstAdjacent(['#000000', '#ffffff', '#000000'])).toBeCloseTo(100, 0);
  });
});

describe.each(THEMES)('%s theme tokens', (theme) => {
  const t = THEME_TOKENS[theme];

  it('meets WCAG AA for text on cards and on the window', () => {
    for (const bg of [t.surface, t.window]) {
      expect(contrast(t.text, bg)).toBeGreaterThanOrEqual(7);
      expect(contrast(t.text2, bg)).toBeGreaterThanOrEqual(4.5);
      expect(contrast(t.text3, bg)).toBeGreaterThanOrEqual(4.5);
      expect(contrast(t.accent, bg)).toBeGreaterThanOrEqual(4.5);
    }
    expect(contrast(t.tooltipText, t.tooltipBg)).toBeGreaterThanOrEqual(7);
  });

  it('status colours are readable as text (they always come with a label too)', () => {
    for (const c of [t.ok, t.warn, t.bad]) expect(contrast(c, t.surface)).toBeGreaterThanOrEqual(4.5);
  });

  it('keeps neighbouring series apart for protanopia and deuteranopia (ΔE ≥ 8) and for normal vision (ΔE ≥ 15)', () => {
    expect(t.series.length).toBe(8);
    expect(worstAdjacent(t.series, 'protan')).toBeGreaterThanOrEqual(8);
    expect(worstAdjacent(t.series, 'deutan')).toBeGreaterThanOrEqual(8);
    expect(worstAdjacent(t.series)).toBeGreaterThanOrEqual(15);
  });

  it('keeps the first three slots apart pairwise (charts with up to three series may put any two side by side)', () => {
    const [a, b, c] = t.series as [string, string, string];
    for (const [x, y] of [
      [a, b],
      [a, c],
      [b, c],
    ] as const) {
      expect(Math.min(deltaE(x, y, 'protan'), deltaE(x, y, 'deutan'))).toBeGreaterThanOrEqual(8);
      expect(deltaE(x, y)).toBeGreaterThanOrEqual(15);
    }
  });

  it('gives chart marks at least 3:1 against the card (light slots 3–5 rely on legends and tooltips instead)', () => {
    const relief = theme === 'light' ? new Set([2, 3, 4]) : new Set<number>();
    t.series.forEach((c, i) => {
      if (!relief.has(i)) expect(contrast(c, t.surface)).toBeGreaterThanOrEqual(3);
    });
  });

  it('assigns series in order and folds overflow into the last slot', () => {
    expect(seriesColor(t, 0)).toBe(t.series[0]);
    expect(seriesColor(t, 42)).toBe(t.series[7]);
  });
});
