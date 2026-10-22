import type { BenchMetric, BenchRun } from '../../core/api.types';
import { mockRun } from '../../core/mock/bench';
import { barFraction, bestValues, kindTitle, statusNote, verdictWord } from './bench-model';

const metric = (id: string, value: number, higherIsBetter = true): BenchMetric => ({ id, label: id, value, unit: 'u', higherIsBetter });

describe('bench model', () => {
  it('names kinds and verdicts in words', () => {
    expect(kindTitle('cpu')).toBe('Processor');
    expect(verdictWord('better')).toBe('Better');
    expect(verdictWord('worse')).toBe('Worse');
    expect(verdictWord('same')).toBe('No real change');
  });

  it('explains every non-finished status and has no note for a finished one', () => {
    expect(statusNote({ state: 'done', metrics: [] })).toBe('');
    expect(statusNote({ state: 'unavailable', reason: 'x' })).toBe('Not available: x');
    expect(statusNote({ state: 'stopped', reason: 'y' })).toBe('Stopped: y');
    expect(statusNote({ state: 'failed', reason: 'z' })).toBe('Failed: z');
  });

  it('bars are relative to the best value, inverted for latency', () => {
    const runs: BenchRun[] = [mockRun('a', 1, 'a', 1), mockRun('b', 2, 'b', 2)];
    const best = bestValues(runs);
    expect(barFraction(metric('cpu-single', best.get('cpu-single')!), best)).toBe(1);
    expect(barFraction(metric('cpu-single', best.get('cpu-single')! / 2), best)).toBeCloseTo(0.5);
    const lat = best.get('mem-latency')!;
    expect(barFraction(metric('mem-latency', lat * 2, false), best)).toBeCloseTo(0.5);
    expect(barFraction(metric('mem-latency', lat, false), best)).toBe(1);
  });

  it('unknown, zero or non-finite values draw no bar', () => {
    const best = new Map([['x', 10]]);
    expect(barFraction(metric('nope', 5), best)).toBe(0);
    expect(barFraction(metric('x', 0), best)).toBe(0);
    expect(barFraction(metric('x', NaN), best)).toBe(0);
    expect(barFraction(metric('x', 50), best)).toBe(1);
  });
});
