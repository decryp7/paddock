import { heat } from './heat';

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
