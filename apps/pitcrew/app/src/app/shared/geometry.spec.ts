import { downsample, sparkPaths, spanBars, timeTicks, timeX } from './geometry';

describe('sparkPaths', () => {
  it('is empty without finite values', () => {
    expect(sparkPaths([], 100, 20)).toEqual({ line: '', area: '', last: null });
    expect(sparkPaths([Number.NaN], 100, 20).last).toBeNull();
  });
  it('centres a single point', () => {
    const p = sparkPaths([5], 100, 20);
    expect(p.last?.x).toBe(50);
    expect(p.line.startsWith('M50.0,')).toBe(true);
  });
  it('spans the width, scaled to the max, with a closed area', () => {
    const p = sparkPaths([0, 10], 100, 22, 10);
    expect(p.line).toBe('M0.0,20.0L100.0,2.0');
    expect(p.area).toBe('M0.0,20.0L100.0,2.0L100.0,22L0.0,22Z');
    expect(p.last).toEqual({ x: 100, y: 2 });
  });
  it('clamps values above the max and defaults max to the peak', () => {
    expect(sparkPaths([20, 5], 10, 22, 10).line).toContain('M0.0,2.0');
    expect(sparkPaths([0, 4], 10, 22).line).toBe('M0.0,20.0L10.0,2.0');
    expect(sparkPaths([1, 2], 10, 22, 0).last).not.toBeNull();
  });
});

describe('downsample', () => {
  it('keeps short series and averages long ones', () => {
    expect(downsample([1, 2, 3], 5)).toEqual([1, 2, 3]);
    expect(downsample([1, 3, 5, 7], 2)).toEqual([2, 6]);
    expect(downsample([1, 2, 3, 4, 5], 3)).toHaveLength(3);
  });
});

describe('spanBars', () => {
  it('scales, clips and drops outside spans, with a 1px minimum', () => {
    const bars = spanBars(
      [
        { from: 0, to: 10 },
        { from: 50, to: 150 },
        { from: 200, to: 300 },
        { from: 20, to: 20.01 },
      ],
      10,
      110,
      200,
    );
    expect(bars).toEqual([
      { x: 80, w: 120 },
      { x: 20, w: 1 },
    ]);
  });
  it('survives an empty range', () => {
    expect(spanBars([{ from: 0, to: 5 }], 1, 1, 100)).toEqual([{ x: 0, w: 1 }]);
  });
});

describe('timeX', () => {
  it('maps inside and rejects outside', () => {
    expect(timeX(50, 0, 100, 200)).toBe(100);
    expect(timeX(-1, 0, 100, 200)).toBeNull();
    expect(timeX(101, 0, 100, 200)).toBeNull();
    expect(timeX(0, 0, 0, 200)).toBe(0);
  });
});

describe('timeTicks', () => {
  const hour = 3_600_000;
  it('uses hour ticks for short ranges', () => {
    const from = new Date(2026, 9, 22, 10, 30).getTime();
    const ticks = timeTicks(from, from + 4 * hour, 400);
    expect(ticks.length).toBeGreaterThanOrEqual(3);
    expect(ticks[0]!.x).toBeGreaterThanOrEqual(0);
    expect(ticks.every((t) => t.x <= 400)).toBe(true);
  });
  it('uses day ticks for long ranges and caps the count', () => {
    const from = new Date(2026, 9, 1, 8).getTime();
    const ticks = timeTicks(from, from + 30 * 24 * hour, 800, 8);
    expect(ticks.length).toBeLessThanOrEqual(9);
    expect(ticks.length).toBeGreaterThan(1);
    expect(ticks[0]!.x).toBeGreaterThan(0);
    const week = timeTicks(from, from + 365 * 24 * hour, 800);
    expect(week.length).toBeGreaterThan(10);
  });
});
