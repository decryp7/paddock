import { cadenceMarks, cadenceTotal } from './cadence';

describe('cadence', () => {
  it('has no marks for an empty series', () => {
    expect(cadenceMarks([])).toEqual([]);
    expect(cadenceTotal([])).toBe(0);
  });

  it('centres the only bucket', () => {
    expect(cadenceMarks([3])).toEqual([{ x: 50, count: 3 }]);
    expect(cadenceMarks([0])).toEqual([]);
  });

  it('spreads buckets from 0 to 100 and skips empty ones', () => {
    expect(cadenceMarks([1, 0, 2, 0, 1])).toEqual([
      { x: 0, count: 1 },
      { x: 50, count: 2 },
      { x: 100, count: 1 },
    ]);
  });

  it('totals the releases', () => {
    expect(cadenceTotal([1, 0, 2, 4])).toBe(7);
  });
});
