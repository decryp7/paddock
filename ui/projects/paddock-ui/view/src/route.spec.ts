import { describe, expect, it } from 'vitest';
import { flattenPath, inflate, pathCrosses, roundedPath, segmentCrosses } from './route';

const box = { x: 10, y: 10, w: 20, h: 20 };

describe('route helpers', () => {
  it('finds a segment through the inside of a box, and not one that only touches it or misses it', () => {
    expect(segmentCrosses([0, 20], [40, 20], box)).toBe(true);
    expect(segmentCrosses([0, 10], [40, 10], box)).toBe(false);
    expect(segmentCrosses([0, 0], [40, 0], box)).toBe(false);
    expect(segmentCrosses([20, 0], [20, 40], box)).toBe(true);
    expect(segmentCrosses([0, 0], [9, 40], box)).toBe(false);
    expect(segmentCrosses([15, 15], [15, 15], box)).toBe(true);
    expect(segmentCrosses([0, 0], [0, 0], box)).toBe(false);
  });
  it('flattens straight, quadratic and cubic commands', () => {
    expect(flattenPath('M 0 0 L 10 0')).toEqual([[0, 0], [10, 0]]);
    const q = flattenPath('M 0 0 Q 10 0 10 10');
    expect(q).toHaveLength(25);
    expect(q[24]).toEqual([10, 10]);
    const c = flattenPath('M 0 0 C 0 10, 10 10, 10 0');
    expect(c[24]![0]).toBeCloseTo(10);
    expect(flattenPath('')).toEqual([]);
  });
  it('tells whether a drawn path crosses a box, curves included', () => {
    expect(pathCrosses('M 0 20 L 40 20', [box])).toBe(true);
    expect(pathCrosses('M 0 0 C 0 40, 40 40, 40 0', [box])).toBe(true);
    expect(pathCrosses('M 0 0 C 0 100, 40 100, 40 0', [box])).toBe(false);
    expect(pathCrosses('M 20 0 Q 20 40 20 40', [box])).toBe(true);
  });
  it('rounds the corners of a polyline and keeps its ends', () => {
    const d = roundedPath([[0, 0], [40, 0], [40, 40]], 8);
    expect(d).toBe('M 0 0 L 32 0 Q 40 0 40 8 L 40 40');
    expect(roundedPath([[0, 0], [10, 0]])).toBe('M 0 0 L 10 0');
    // A repeated point has no corner to round.
    expect(roundedPath([[0, 0], [0, 0], [10, 0]])).toBe('M 0 0 L 10 0');
  });
  it('grows a box on every side', () => {
    expect(inflate(box, 3)).toEqual({ x: 7, y: 7, w: 26, h: 26 });
  });
});
