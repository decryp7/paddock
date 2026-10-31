import type { Series } from '../../state/live.store';
import { seriesFor } from './series';

const series = (over: Partial<Series> = {}): Series => ({
  cpu: [10, 20],
  memory: [50, 51],
  diskRead: [3e8, 5e8],
  diskWrite: [1e8, 2e8],
  diskBusy: [40, 60],
  netRx: [1e6, 2e6],
  netTx: [1e5, 2e5],
  gpu: [5, 6],
  ...over,
});

describe('seriesFor', () => {
  it('draws disk activity from the busy percentage, the unit of its reading, and not from bytes a second', () => {
    const s = seriesFor('diskActivity', series());
    expect(s).toEqual({ values: [40, 60], values2: null });
    // The bytes series are in another unit: a line capped at 100 % must never be fed them.
    expect(s?.values.every((v) => v <= 100)).toBe(true);
  });

  it('has no line for a disk that reports no busy time', () => {
    expect(seriesFor('diskActivity', series({ diskBusy: [] }))?.values).toEqual([]);
  });

  it('keeps the network in bytes a second (its reading is a rate, not a percentage)', () => {
    expect(seriesFor('networkDown', series())).toEqual({ values: [1e6, 2e6], values2: null });
  });
});
