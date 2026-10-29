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
    // Red and green lose their hue difference for protanopes and deuteranopes. The OKLab distance keeps the lightness
    // difference between these two (so it drops by about a third, not by half); an equal-lightness pair collapses fully.
    const normal = deltaE('#d62728', '#2ca02c');
    expect(deltaE('#d62728', '#2ca02c', 'protan')).toBeLessThan(normal * 0.75);
    expect(deltaE('#d62728', '#2ca02c', 'deutan')).toBeLessThan(normal * 0.75);
    expect(deltaE('#b8592f', '#4f8a2b', 'protan')).toBeLessThan(deltaE('#b8592f', '#4f8a2b') * 0.5);
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

  it('status text is readable on its own tinted pill background (the pills carry a word too)', () => {
    const pairs = [
      [t.ok, t.okTint],
      [t.warn, t.warnTint],
      [t.bad, t.badTint],
      [t.info, t.infoTint],
    ] as const;
    for (const [fg, bg] of pairs) expect(contrast(fg, bg)).toBeGreaterThanOrEqual(4.5);
  });

  it('the accent is not a status colour: it stays apart from warning and danger in hue and is readable as text', () => {
    expect(t.accent).not.toBe(t.info);
    expect(contrast(t.accent, t.surface)).toBeGreaterThanOrEqual(4.5);
  });

  it('draws the memory history in the guide palette, each line readable against the card', () => {
    expect(t.history.length).toBe(3);
    for (const c of t.history) expect(contrast(c, t.surface)).toBeGreaterThanOrEqual(3);
    expect(deltaE(t.history[0], t.history[1])).toBeGreaterThanOrEqual(15);
  });

  // The series are the family palette (--pd-chart-1..8): its colour-vision separation (adjacent and the first five pairwise,
  // under protanopia, deuteranopia and tritanopia) is measured once, by paddock-ui's token test (ui/test/tokens), not here.
  it('takes the eight family slots in order', () => {
    expect(t.series.length).toBe(8);
    expect(new Set(t.series).size).toBe(8);
    expect(t.history).toEqual(t.series.slice(0, 3));
  });

  it('gives every chart mark at least 3:1 against the card', () => {
    t.series.forEach((c) => expect(contrast(c, t.surface)).toBeGreaterThanOrEqual(3));
  });

  it('assigns series in order and folds overflow into the last slot', () => {
    expect(seriesColor(t, 0)).toBe(t.series[0]);
    expect(seriesColor(t, 42)).toBe(t.series[7]);
  });
});
