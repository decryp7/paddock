import { TestBed } from '@angular/core/testing';
import { SuSparkline, headroomMax, sparklineGeometry, sparklineScale, zoomRange } from './sparkline';

/** The numbers of a path, in order: the golden compares coordinates, not the number of decimals. */
const nums = (d: string): number[] => (d.match(/-?\d+(\.\d+)?/g) ?? []).map(Number);
const commands = (d: string): string => d.replace(/[^MLZ]/g, '');

/**
 * Golden paths recorded from Dyno's own sparkline (apps/dyno/app/src/app/shared/sparkline.ts before it moved here), in its 100 x 100
 * box with two decimals. The shared geometry at 100 x 100 with no padding must draw the same lines (within the rounding to one decimal).
 */
const CPU = [20, 57, 34, 71, 48, 25, 62, 39, 76, 53, 30, 67];
const MEM = [50, 50.5, 51, 50.8, 51.2, 52];
const RX = [0, 1200, 3400, 800, 15000, 9000, 200];
const TX = [0, 300, 100, 50, 4000, 2000, 10];
const GOLDEN = {
  cpu: 'M81.36 80.00L83.05 43.00L84.75 66.00L86.44 29.00L88.14 52.00L89.83 75.00L91.53 38.00L93.22 61.00L94.92 24.00L96.61 47.00L98.31 70.00L100.00 33.00',
  mem: 'M91.53 62.50L93.22 56.25L94.92 50.00L96.61 52.50L98.31 47.50L100.00 37.50',
  rx: 'M89.83 100.00L91.53 93.04L93.22 80.29L94.92 95.36L96.61 13.04L98.31 47.83L100.00 98.84',
  tx: 'M89.83 100.00L91.53 98.26L93.22 99.42L94.92 99.71L96.61 76.81L98.31 88.41L100.00 99.94',
  trend: 'M0.00 50.00L25.00 42.31L50.00 11.54L75.00 30.77L100.00 88.46',
  zeros: 'M96.61 100.00L98.31 100.00L100.00 100.00',
};

function expectSame(actual: string, golden: string): void {
  expect(commands(actual)).toBe(commands(golden));
  const a = nums(actual);
  const g = nums(golden);
  expect(a.length).toBe(g.length);
  a.forEach((n, i) => expect(Math.abs(n - (g[i] as number))).toBeLessThanOrEqual(0.051));
}

describe('sparkline geometry against Dyno goldens', () => {
  const draw = (v: readonly number[], s: { min?: number; max?: number }, capacity: number) =>
    sparklineGeometry(v, 100, 100, 0, s.min, s.max, { capacity, clamp: true });

  it('a fixed 0..100 level, right-aligned in 60 slots (CPU)', () => {
    expectSame(draw(CPU, sparklineScale(CPU, null, 'zero', 100), 60).line, GOLDEN.cpu);
  });
  it('a zoomed level (memory)', () => {
    expectSame(draw(MEM, sparklineScale(MEM, null, 'zoom', 100), 60).line, GOLDEN.mem);
  });
  it('two series share one scale with headroom (network)', () => {
    const s = sparklineScale(RX, TX, 'zero');
    expectSame(draw(RX, s, 60).line, GOLDEN.rx);
    expectSame(draw(TX, s, 60).line, GOLDEN.tx);
  });
  it('a zoomed trend that fills the box (capacity = length)', () => {
    const t = [80, 82, 90, 85, 70];
    expectSame(draw(t, sparklineScale(t, null, 'zoom', 100), 5).line, GOLDEN.trend);
  });
  it('all zeros draw along the bottom, not the middle', () => {
    const z = [0, 0, 0];
    expectSame(draw(z, sparklineScale(z, null, 'zero'), 60).line, GOLDEN.zeros);
  });
});

describe('sparklineGeometry options', () => {
  it('keeps the old output without options', () => {
    expect(sparklineGeometry([0, 10], 100, 30, 3).line).toBe('M3.0 27.0L97.0 3.0');
  });
  it('right-aligns fewer values than the capacity and ends at the right edge', () => {
    const g = sparklineGeometry([10, 10], 100, 30, 0, 0, 10, { capacity: 3 });
    expect(g.line).toBe('M50.0 0.0L100.0 0.0');
    expect(g.end).toEqual({ x: 100, y: 0 });
  });
  it('grows the box past a too-small capacity', () => {
    expect(sparklineGeometry([0, 10, 0], 100, 30, 0, 0, 10, { capacity: 1 }).line).toBe('M0.0 30.0L50.0 0.0L100.0 30.0');
  });
  it('ignores a capacity that is not a number', () => {
    expect(sparklineGeometry([1, 2], 100, 30, 0, 0, 2, { capacity: Number.NaN }).line).toBe('M0.0 15.0L100.0 0.0');
  });
  it('clamps only when asked', () => {
    expect(sparklineGeometry([-5, 50], 100, 100, 0, 0, 10).line).toBe('M0.0 150.0L100.0 -400.0');
    expect(sparklineGeometry([-5, 50], 100, 100, 0, 0, 10, { clamp: true }).line).toBe('M0.0 100.0L100.0 0.0');
  });
  it('has no area for a single point', () => {
    expect(sparklineGeometry([4], 100, 30).area).toBe('');
  });
  it('a scale with no or a negative span draws mid-height', () => {
    expect(sparklineGeometry([5, 5], 100, 30, 0, 5, 3).line).toBe('M0.0 15.0L100.0 15.0');
    expect(sparklineGeometry([5, 5], 100, 30, 0, 5, 5).line).toBe('M0.0 15.0L100.0 15.0');
  });
});

describe('zoomRange', () => {
  it('widens a flat level to the minimum span around it', () => {
    expect(zoomRange([50, 50, 50], 8)).toEqual({ min: 46, max: 54 });
  });
  it('follows the data with padding when it moves more than the minimum', () => {
    const r = zoomRange([40, 60], 8);
    expect(r.min).toBeCloseTo(37);
    expect(r.max).toBeCloseTo(63);
  });
  it('stays inside 0 and the ceiling and handles no data', () => {
    expect(zoomRange([2, 2], 8)).toEqual({ min: 0, max: 8 });
    expect(zoomRange([99, 99], 8)).toEqual({ min: 92, max: 100 });
    expect(zoomRange([], 8)).toEqual({ min: 0, max: 100 });
  });
  it('never throws on NaN and ignores it', () => {
    expect(zoomRange([Number.NaN], 8)).toEqual({ min: 0, max: 100 });
    expect(zoomRange([50, Number.NaN, 50], 8)).toEqual({ min: 46, max: 54 });
  });
  it('uses the defaults', () => {
    expect(zoomRange([50])).toEqual({ min: 46, max: 54 });
  });
});

describe('headroomMax', () => {
  it('prefers the fixed max', () => {
    expect(headroomMax([[1, 2]], 100)).toBe(100);
  });
  it('adds 15% headroom to the data of every series', () => {
    expect(headroomMax([[1, 2], [4]])).toBeCloseTo(4.6);
  });
  it('falls back to 1 for empty, all-zero or NaN data', () => {
    expect(headroomMax([])).toBe(1);
    expect(headroomMax([[0, 0]])).toBe(1);
    expect(headroomMax([[Number.NaN]])).toBe(1);
  });
});

describe('sparklineScale', () => {
  it('fit follows the data and ignores max', () => {
    expect(sparklineScale([1, 2], null, 'fit')).toEqual({ min: 1, max: 2 });
    expect(sparklineScale([1, 2], null, 'fit', 9)).toEqual({ min: 1, max: 2 });
    expect(sparklineScale([], [], 'fit')).toEqual({});
    expect(sparklineScale([Number.NaN], undefined, 'fit', 9)).toEqual({});
  });
  it('fit puts two series on one range, with or without a max', () => {
    expect(sparklineScale([1, 2], [0, 5], 'fit')).toEqual({ min: 0, max: 5 });
    expect(sparklineScale([1, 2], [0, 5], 'fit', 9)).toEqual({ min: 0, max: 5 });
  });
  it('zero starts at 0 and counts both series in the headroom', () => {
    expect(sparklineScale([1], [10], 'zero')).toEqual({ min: 0, max: 11.5 });
    expect(sparklineScale([1], undefined, 'zero', 4)).toEqual({ min: 0, max: 4 });
  });
  it('zoom follows the first series inside the ceiling', () => {
    expect(sparklineScale([50, 50], [99], 'zoom')).toEqual({ min: 46, max: 54 });
    expect(sparklineScale([99, 99], null, 'zoom', 100)).toEqual({ min: 92, max: 100 });
  });
});

describe('SuSparkline options', () => {
  function make(inputs: Record<string, unknown>): HTMLElement {
    const f = TestBed.createComponent(SuSparkline);
    for (const [k, v] of Object.entries(inputs)) f.componentRef.setInput(k, v);
    f.detectChanges();
    return f.nativeElement as HTMLElement;
  }
  const lines = (el: HTMLElement): string[] => Array.from(el.querySelectorAll('.pd-line')).map((p) => p.getAttribute('d') ?? '');
  const EDGE = { pad: 0, height: 10 };

  it('draws exact paths from zero to a fixed max', () => {
    expect(lines(make({ values: [0, 10], scale: 'zero', max: 10, ...EDGE }))).toEqual(['M0.0 10.0L120.0 0.0']);
  });
  it('a fitted line follows the data and ignores max; a zero line is clamped to its max', () => {
    expect(lines(make({ values: [-5, 50], max: 10, ...EDGE }))[0]).toBe('M0.0 10.0L120.0 0.0');
    expect(lines(make({ values: [-5, 50], scale: 'zero', max: 10, ...EDGE }))[0]).toBe('M0.0 10.0L120.0 0.0');
  });
  it('draws a dashed second line on the same scale, clamped into a zoomed range', () => {
    const el = make({ values: [50, 52], values2: [90, 10], scale: 'zoom', pad: 0, height: 8, color2: 'rgb(1, 2, 3)' });
    expect(lines(el)).toEqual(['M0.0 5.0L120.0 3.0', 'M0.0 0.0L120.0 8.0']);
    expect((el.querySelector('.pd-line2') as SVGPathElement).style.stroke).toBe('rgb(1, 2, 3)');
  });
  it('counts the second series in the scale from zero', () => {
    expect(lines(make({ values: [0, 5], values2: [0, 10], scale: 'zero', ...EDGE }))).toEqual(['M0.0 10.0L120.0 5.7', 'M0.0 10.0L120.0 1.3']);
  });
  it('has no second line without a second series', () => {
    expect(make({ values: [1, 2] }).querySelector('.pd-line2')).toBeNull();
    expect(make({ values: [1, 2], values2: [] }).querySelector('.pd-line2')).toBeNull();
    expect(make({ values: [1, 2], values2: undefined }).querySelector('.pd-line2')).toBeNull();
  });
  it('colours the line from the colour input', () => {
    expect(make({ values: [1, 2], color: 'rgb(9, 8, 7)' }).style.color).toBe('rgb(9, 8, 7)');
    expect(make({ values: [1, 2] }).style.color).toBe('');
  });
  it('can hide the end dot and run edge to edge', () => {
    const el = make({ values: [1, 2], dot: false, scale: 'zero', max: 2, ...EDGE });
    expect(el.querySelector('.pd-dot')).toBeNull();
    expect(lines(el)).toEqual(['M0.0 5.0L120.0 0.0']);
  });
  it('grows in from the right with a capacity', () => {
    expect(lines(make({ values: [1, 2], capacity: 3, scale: 'zero', max: 2, ...EDGE }))).toEqual(['M60.0 5.0L120.0 0.0']);
  });
  it('draws an area under a zoomed line', () => {
    const el = make({ values: [50, 52], scale: 'zoom', area: true, ...EDGE });
    expect(el.querySelector('.pd-area')?.getAttribute('d')).toMatch(/Z$/);
  });

  it('names the latest and peak values rounded, never as raw floats', () => {
    const name = make({ values: [50.9375, 53.43750000000001], label: 'Memory' }).querySelector('svg')?.getAttribute('aria-label') ?? '';
    expect(name).toMatch(/^Memory: latest 53\.4, peak 53\.4$/);
  });
  it('formats the values with the given function and names the second series', () => {
    const el = make({ values: [1, 4], values2: [2, 3], label: 'Down', label2: 'Up', formatValue: (v: number) => `${v} MB/s` });
    expect(el.querySelector('svg')?.getAttribute('aria-label')).toBe('Down: latest 4 MB/s, peak 4 MB/s; Up: latest 3 MB/s, peak 3 MB/s');
  });
  it('leaves an empty second series out of the name', () => {
    expect(make({ values: [1], values2: [Number.NaN], label: 'A' }).querySelector('svg')?.getAttribute('aria-label')).toBe('A: latest 1, peak 1');
  });
  it('says there is no data for an empty series', () => {
    expect(make({ values: [], label: 'A' }).querySelector('svg')?.getAttribute('aria-label')).toBe('A: no data');
  });

  it('never throws or draws NaN for odd inputs', () => {
    const odd = { values: [1, Number.NaN, 3], values2: [Number.NaN], capacity: Number.NaN, pad: Number.NaN, max: Number.NaN, height: Number.NaN, area: true };
    for (const scale of ['fit', 'zero', 'zoom']) {
      const el = make({ ...odd, scale });
      expect(el.innerHTML).not.toContain('NaN');
      expect(lines(el)[0]).toMatch(/^M/);
    }
    expect(make({ values: [1, 2], capacity: 0, pad: -1 }).innerHTML).not.toContain('NaN');
    expect(make({ values: [1, 2], pad: 80 }).innerHTML).not.toContain('NaN');
  });
});
