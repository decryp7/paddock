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
    expectSame(draw(CPU, sparklineScale(CPU, null, { max: 100, fromZero: true }), 60).line, GOLDEN.cpu);
  });
  it('a zoomed level (memory)', () => {
    expectSame(draw(MEM, sparklineScale(MEM, null, { max: 100, zoom: true }), 60).line, GOLDEN.mem);
  });
  it('two series share one scale with headroom (network)', () => {
    const s = sparklineScale(RX, TX, { fromZero: true });
    expectSame(draw(RX, s, 60).line, GOLDEN.rx);
    expectSame(draw(TX, s, 60).line, GOLDEN.tx);
  });
  it('a zoomed trend that fills the box (capacity = length)', () => {
    const t = [80, 82, 90, 85, 70];
    expectSame(draw(t, sparklineScale(t, null, { max: 100, zoom: true }), 5).line, GOLDEN.trend);
  });
  it('all zeros draw along the bottom, not the middle', () => {
    const z = [0, 0, 0];
    expectSame(draw(z, sparklineScale(z, null, { fromZero: true }), 60).line, GOLDEN.zeros);
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
  it('clamps only when asked', () => {
    expect(sparklineGeometry([-5, 50], 100, 100, 0, 0, 10).line).toBe('M0.0 150.0L100.0 -400.0');
    expect(sparklineGeometry([-5, 50], 100, 100, 0, 0, 10, { clamp: true }).line).toBe('M0.0 100.0L100.0 0.0');
  });
  it('has no area for a single point', () => {
    expect(sparklineGeometry([4], 100, 30).area).toBe('');
  });
  it('a scale with no span draws mid-height', () => {
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
  it('passes a fixed scale through and leaves a missing side to the data', () => {
    expect(sparklineScale([1, 2], null, { min: 0, max: 10 })).toEqual({ min: 0, max: 10 });
    expect(sparklineScale([1, 2], null, {})).toEqual({ min: undefined, max: undefined });
  });
  it('fits two series to one scale', () => {
    expect(sparklineScale([1, 2], [0, 5], {})).toEqual({ min: 0, max: 5 });
    expect(sparklineScale([1, 2], [0, 5], { min: 1 })).toEqual({ min: 1, max: 5 });
    expect(sparklineScale([1, 2], [0, 5], { min: 1, max: 9 })).toEqual({ min: 1, max: 9 });
    expect(sparklineScale([], [], {})).toEqual({ min: undefined, max: undefined });
  });
});

describe('SuSparkline options', () => {
  function make(inputs: Record<string, unknown>): HTMLElement {
    const f = TestBed.createComponent(SuSparkline);
    for (const [k, v] of Object.entries(inputs)) f.componentRef.setInput(k, v);
    f.detectChanges();
    return f.nativeElement as HTMLElement;
  }
  it('draws a dashed second line only when given', () => {
    expect(make({ values: [1, 2] }).querySelector('.pd-line2')).toBeNull();
    expect(make({ values: [1, 2], values2: [] }).querySelector('.pd-line2')).toBeNull();
    const el = make({ values: [1, 2], values2: [2, 1], color2: 'rgb(1, 2, 3)' });
    const p = el.querySelector('.pd-line2') as SVGPathElement;
    expect(p.getAttribute('d')).toMatch(/^M/);
    expect(p.style.stroke).toBe('rgb(1, 2, 3)');
  });
  it('colours the line from the colour input', () => {
    expect(make({ values: [1, 2], color: 'rgb(9, 8, 7)' }).style.color).toBe('rgb(9, 8, 7)');
    expect(make({ values: [1, 2] }).style.color).toBe('');
  });
  it('can hide the end dot and run edge to edge', () => {
    const el = make({ values: [1, 2], dot: false, pad: 0, min: 0, max: 2, height: 10 });
    expect(el.querySelector('.pd-dot')).toBeNull();
    expect(el.querySelector('.pd-line')?.getAttribute('d')).toBe('M0.0 5.0L120.0 0.0');
  });
  it('grows in from the right with a capacity', () => {
    const el = make({ values: [1, 2], capacity: 3, pad: 0, fromZero: true, max: 2 });
    expect(el.querySelector('.pd-line')?.getAttribute('d')).toMatch(/^M60\.0 /);
  });
  it('zooms and still draws an area from a zoomed range', () => {
    const el = make({ values: [50, 52], zoom: true, area: true, pad: 0 });
    expect(el.querySelector('.pd-area')?.getAttribute('d')).toMatch(/Z$/);
    expect(el.querySelector('.pd-line')?.getAttribute('d')).not.toContain('100.0 ');
  });
  it('renders a defined state for odd input', () => {
    const el = make({ values: [Number.NaN, Number.NaN], values2: [Number.NaN], zoom: true, area: true });
    expect(el.querySelector('svg')?.getAttribute('aria-label')).toContain('no data');
    expect(el.querySelector('.pd-line2')).toBeNull();
  });
});
