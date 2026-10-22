import { TestBed } from '@angular/core/testing';
import { Sparkline, sparkMax, sparkPath, zoomRange } from './sparkline';

describe('sparkPath', () => {
  it('is empty without values or a positive scale', () => {
    expect(sparkPath([], 10, 100)).toBe('');
    expect(sparkPath([1], 0, 100)).toBe('');
  });
  it('draws left to right, 0 at the bottom', () => {
    expect(sparkPath([0, 10], 10, 100)).toBe('M0.00 100.00L100.00 0.00');
  });
  it('clamps values into the box', () => {
    expect(sparkPath([-5, 50], 10, 100)).toBe('M0.00 100.00L100.00 0.00');
  });
  it('right-aligns fewer values than the capacity', () => {
    expect(sparkPath([10, 10], 10, 100, 3)).toBe('M50.00 0.00L100.00 0.00');
  });
  it('handles a single slot', () => {
    expect(sparkPath([5], 10, 100, 1)).toBe('M0.00 50.00');
  });
  it('grows the box past a too-small capacity', () => {
    expect(sparkPath([0, 10, 0], 10, 100, 1)).toBe('M0.00 100.00L50.00 0.00L100.00 100.00');
  });
});

describe('sparkMax', () => {
  it('prefers the fixed max', () => {
    expect(sparkMax([[1, 2]], 100)).toBe(100);
  });
  it('adds 15% headroom to the data', () => {
    expect(sparkMax([[1, 2], [4]], null)).toBeCloseTo(4.6);
  });
  it('falls back to 1 for empty or all-zero data', () => {
    expect(sparkMax([], null)).toBe(1);
    expect(sparkMax([[0, 0]], null)).toBe(1);
  });
});

describe('Sparkline component', () => {
  function make(values: number[], values2: number[] | null = null, fill = true) {
    const f = TestBed.createComponent(Sparkline);
    f.componentRef.setInput('values', values);
    f.componentRef.setInput('values2', values2);
    f.componentRef.setInput('fill', fill);
    f.componentRef.setInput('label', 'CPU');
    f.detectChanges();
    return f.nativeElement as HTMLElement;
  }

  it('draws a line and an area for two or more samples', () => {
    const el = make([1, 2, 3]);
    expect(el.querySelector('path.line')?.getAttribute('d')).toMatch(/^M/);
    expect(el.querySelector('path.area')?.getAttribute('d')).toMatch(/Z$/);
    expect(el.querySelector('path.second')).toBeNull();
    expect(el.getAttribute('aria-label')).toBe('CPU');
  });
  it('omits the area for one sample or when fill is off', () => {
    expect(make([1]).querySelector('path.area')).toBeNull();
    expect(make([1, 2], null, false).querySelector('path.area')).toBeNull();
  });
  it('draws a second line when given', () => {
    expect(make([1, 2], [2, 1]).querySelector('path.second')).not.toBeNull();
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
  it('draws a zoomed line with slope instead of a flat edge', () => {
    const { min, max } = zoomRange([50, 52], 8);
    const d = sparkPath([50, 52], max, 100, 2, min);
    expect(d).not.toContain('100.00L100.00 100.00');
    expect(d.startsWith('M0.00')).toBe(true);
  });
});
