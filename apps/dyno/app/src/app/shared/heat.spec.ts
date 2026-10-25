import { bar, heat } from './heat';

describe('heat', () => {
  it('gives nothing for zero, negative, non-finite or bad scales', () => {
    expect(heat(0, 10)).toBeNull();
    expect(heat(-1, 10)).toBeNull();
    expect(heat(NaN, 10)).toBeNull();
    expect(heat(5, 0)).toBeNull();
  });
  it('is faint for small values and strongest at full scale', () => {
    expect(heat(10, 10)).toBe('rgb(var(--heat) / 0.3)');
    expect(heat(100, 10)).toBe('rgb(var(--heat) / 0.3)');
    expect(heat(2.5, 10)).toBe('rgb(var(--heat) / 0.17)');
  });
  it('grows monotonically', () => {
    const alpha = (v: number) => Number(/\/ ([\d.]+)\)/.exec(heat(v, 100)!)![1]);
    expect(alpha(1)).toBeLessThan(alpha(10));
    expect(alpha(10)).toBeLessThan(alpha(50));
  });
});

describe('bar', () => {
  it('gives nothing for zero, negative, non-finite or bad scales', () => {
    for (const v of [0, -1, NaN]) expect(bar(v, 10)).toBeNull();
    expect(bar(5, 0)).toBeNull();
  });
  it('fills in proportion, capped at the full scale', () => {
    expect(bar(5, 10)).toContain('50%');
    expect(bar(50, 10)).toContain('100%');
  });
  it('is grey unless the column is the sort column', () => {
    expect(bar(5, 10)).toContain('var(--text-3)');
    expect(bar(5, 10, true)).toContain('var(--heat)');
  });
});
