import type { BenchMetric, BenchRun } from '../../core/api.types';
import { mockRun } from '../../core/mock/bench';
import { barPair, barFraction, bestValues, kindTitle, livenessNote, methodNote, phaseText, spreadText, statusNote, verdictWord } from './bench-model';

const metric = (id: string, value: number, higherIsBetter = true): BenchMetric => ({ id, label: id, value, unit: 'u', higherIsBetter, samples: [value], min: value, max: value, cv: 0, unsteady: false });

describe('bench model', () => {
  it('says where a phase is, and how a slow or quiet run is shown', () => {
    expect(phaseText({ phase: 'cpu', label: 'Warming up', rep: 0, of: 5, count: 0, total: 5 })).toBe('0 of 5 passes done');
    expect(phaseText({ phase: 'cpu', label: 'Cooling down before pass 3 of 5', rep: 2, of: 5, count: 2, total: 5 })).toBe('2 of 5 passes done');
    expect(livenessNote('running')).toBe('');
    expect(livenessNote('stillWorking')).toContain('Still working');
    expect(livenessNote('notResponding')).toContain('Not responding');
    expect(livenessNote('notResponding')).toContain('Cancel');
  });

  it('shows the spread of repetitions in words, and nothing for a single measurement', () => {
    const fmt = (v: number): string => String(v);
    expect(spreadText({ ...metric('x', 100), samples: [98, 100, 102], min: 98, max: 102 }, fmt)).toBe('98 to 102');
    expect(spreadText(metric('x', 100), fmt)).toBe('');
    expect(spreadText({ ...metric('x', 100), samples: [1, 2], min: NaN, max: 2 }, fmt)).toBe('');
  });

  it('marks a run measured the old way and no other', () => {
    expect(methodNote({ ...mockRun('a', 1, 'a', 1), oldWay: true })).toContain('old way');
    expect(methodNote(mockRun('a', 1, 'a', 1))).toBe('');
  });

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

  it('a bar and its earlier-best marker share one scale, inverted for latency', () => {
    expect(barPair(metric('x', 120), 100)).toEqual({ fill: 1, marker: 100 / 120 });
    expect(barPair(metric('x', 80), 100)).toEqual({ fill: 0.8, marker: 1 });
    expect(barPair(metric('lat', 50, false), 100)).toEqual({ fill: 1, marker: 0.5 });
    expect(barPair(metric('x', 5), undefined)).toEqual({ fill: 1, marker: null });
    expect(barPair(metric('x', 0), 5)).toEqual({ fill: 0, marker: null });
  });
});
