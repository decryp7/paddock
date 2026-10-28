import { GiB } from '../../core/mock-data';
import { barWidth, limitValue, tokensOf } from './usage-format';

describe('usage format', () => {
  it('formats limit values in their own unit', () => {
    expect(limitValue('memoryShare', 4 * GiB)).toBe('4 GB');
    expect(limitValue('tokensPerDay', 5_000_000)).toBe('5.0M');
    expect(limitValue('tokensPerMonth', 1500)).toBe('1.5k');
    expect(limitValue('requestsPerDay', 12_345)).toBe('12,345');
    expect(limitValue('requestsPerMin', 60)).toBe('60');
  });

  it('caps a bar at its full width and never goes negative', () => {
    expect([-5, 0, 42, 100, 250].map(barWidth)).toEqual([0, 0, 42, 100, 100]);
  });

  it('adds prompt and completion tokens', () => {
    expect(tokensOf({ requests: 1, promptTokens: 7, completionTokens: 5, busyMs: 0, errors: 0, peakMemoryBytes: 0, cacheHits: 0, cacheMisses: 0, prefixTokens: 0, savedTokens: 0 })).toBe(12);
  });
});
