import { bar, coreHeat, heat } from './heat';

describe('heat', () => {
  it('gives nothing for zero, negative, non-finite or bad scales', () => {
    expect(heat(0, 10)).toBeNull();
    expect(heat(-1, 10)).toBeNull();
    expect(heat(NaN, 10)).toBeNull();
    expect(heat(5, 0)).toBeNull();
  });
  it('is faint for small values and strongest at full scale', () => {
    expect(heat(10, 10)).toBe('color-mix(in srgb, var(--pd-accent) 30%, transparent)');
    expect(heat(100, 10)).toBe('color-mix(in srgb, var(--pd-accent) 30%, transparent)');
    expect(heat(2.5, 10)).toBe('color-mix(in srgb, var(--pd-accent) 17%, transparent)');
  });
  it('grows monotonically', () => {
    const alpha = (v: number) => Number(/ (\d+)%/.exec(heat(v, 100)!)![1]);
    expect(alpha(1)).toBeLessThan(alpha(10));
    expect(alpha(10)).toBeLessThan(alpha(50));
  });
});

describe('bar', () => {
  it('gives nothing for zero, negative, non-finite or bad scales', () => {
    for (const v of [0, -1, NaN]) expect(bar(v, 10)).toBeNull();
    expect(bar(5, 0)).toBeNull();
  });
  it('gives nothing for a value too small to read as a bar (a 1% sliver looks like a column divider)', () => {
    expect(bar(0.2, 40)).toBeNull();
    expect(bar(0.8, 40)).toBeNull();
    expect(bar(2, 40)).toContain('5%');
  });
  it('fills in proportion, capped at the full scale', () => {
    expect(bar(5, 10)).toContain('50%');
    expect(bar(50, 10)).toContain('100%');
  });
  it('is grey unless the column is the sort column', () => {
    expect(bar(5, 10)).toContain('var(--text-3)');
    expect(bar(5, 10, true)).toContain('var(--pd-accent)');
  });
});

describe('coreHeat', () => {
  it('starts every cell from a neutral that is 3:1 on a card (WCAG 1.4.11), so an idle core is still a visible cell', () => {
    expect(coreHeat(0)).toBe('color-mix(in srgb, var(--pd-accent-control) 25%, var(--pd-text-subtle))');
    expect(coreHeat(NaN)).toBe(coreHeat(0));
    expect(coreHeat(-20)).toBe(coreHeat(0));
  });
  it('moves toward the 3:1 accent control colour as the load grows and stops at full', () => {
    expect(coreHeat(100)).toBe('color-mix(in srgb, var(--pd-accent-control) 100%, var(--pd-text-subtle))');
    expect(coreHeat(400)).toBe(coreHeat(100));
    expect(coreHeat(50)).toBe('color-mix(in srgb, var(--pd-accent-control) 63%, var(--pd-text-subtle))');
  });
});
