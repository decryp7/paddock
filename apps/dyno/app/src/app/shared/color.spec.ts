import { contrast, deltaE, luminance, parseHex, simulate, toLab, withAlpha } from './color';

describe('colour maths', () => {
  it('parses hex and rejects anything else', () => {
    expect(parseHex('#ff8000')).toEqual([255, 128, 0]);
    expect(parseHex('00ff00')).toEqual([0, 255, 0]);
    expect(() => parseHex('red')).toThrow();
  });

  it('computes WCAG luminance and contrast', () => {
    expect(luminance([255, 255, 255])).toBeCloseTo(1);
    expect(luminance([0, 0, 0])).toBe(0);
    expect(contrast('#000000', '#ffffff')).toBeCloseTo(21);
    expect(contrast('#ffffff', '#000000')).toBeCloseTo(21);
    expect(contrast('#777777', '#777777')).toBeCloseTo(1);
  });

  it('simulates colour-vision deficiency (grey stays grey, red and green converge for deuteranopia)', () => {
    expect(simulate('#808080', 'deuteranopia').every((c) => Math.abs(c - 128) <= 2)).toBe(true);
    const normal = deltaE(parseHex('#d62728'), parseHex('#2ca02c'));
    const deut = deltaE(simulate('#d62728', 'deuteranopia'), simulate('#2ca02c', 'deuteranopia'));
    expect(deut).toBeLessThan(normal / 2);
    expect(simulate('#0000ff', 'tritanopia')).not.toEqual([0, 0, 255]);
    expect(simulate('#ff0000', 'protanopia')).not.toEqual([255, 0, 0]);
  });

  it('converts to Lab and measures distance', () => {
    expect(toLab([255, 255, 255])[0]).toBeCloseTo(100, 0);
    expect(toLab([0, 0, 0])[0]).toBeCloseTo(0, 5);
    expect(toLab([1, 1, 1])[0]).toBeGreaterThan(0);
    expect(deltaE([10, 20, 30], [10, 20, 30])).toBe(0);
  });

  it('formats rgba', () => {
    expect(withAlpha('#102030', 0.5)).toBe('rgba(16, 32, 48, 0.5)');
  });
});
