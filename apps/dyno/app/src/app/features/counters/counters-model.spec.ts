import { describe, expect, it } from 'vitest';
import type { CounterHistory, CounterReading, Widget } from '../../core/api.types';
import { LIGHT } from '../../shared/tokens';
import { counterByteKind, counterChartOption, counterText, counterTitle, formatCounter, pathParts, readingNumber, readingReason, widgetCounterPaths, withPath, withoutPath } from './counters-model';

const W = (metric: Widget['metric']): Widget => ({ id: 'x', kind: 'graph', metric, size: 's', options: { rangeSecs: 300, showLabel: true, fill: true, accent: null }, thresholds: { warning: null, critical: null } });

describe('counter paths', () => {
  it('splits for display', () => {
    expect(pathParts('\\Memory\\Pages/sec')).toEqual({ object: 'Memory', instance: null, counter: 'Pages/sec' });
    expect(pathParts('\\Network Interface(Intel(R) Ethernet)\\Bytes Sent/sec')).toEqual({ object: 'Network Interface', instance: 'Intel(R) Ethernet', counter: 'Bytes Sent/sec' });
    expect(pathParts('junk')).toEqual({ object: 'junk', instance: null, counter: '' });
  });

  it('titles a row with its instance', () => {
    expect(counterTitle('\\PhysicalDisk(0 C:)\\Disk Read Bytes/sec')).toBe('Disk Read Bytes/sec (0 C:)');
    expect(counterTitle('\\Memory\\Pages/sec')).toBe('Pages/sec');
  });
});

describe('values', () => {
  it('formats each unit with the unit kept apart', () => {
    expect(formatCounter(34.2, 'percent')).toEqual(['34', '%']);
    expect(formatCounter(2000, 'bytes')).toEqual(['2', 'KB']);
    expect(formatCounter(1500000, 'bytesPerSec')).toEqual(['1.5', 'MB/s']);
    expect(formatCounter(412.4, 'perSec')).toEqual(['412', '/s']);
    expect(formatCounter(0.456, 'milliseconds')).toEqual(['0.46', 'ms']);
    expect(formatCounter(12.34, 'seconds')).toEqual(['12.3', 's']);
    expect(formatCounter(24000, 'number')).toEqual(['24,000', '']);
    expect(formatCounter(3, 'count')).toEqual(['3', '']);
    expect(counterText(1500000, 'bytesPerSec')).toBe('1.5 MB/s');
    expect(counterText(3, 'count')).toBe('3');
  });

  it('reads readings', () => {
    const ok: CounterReading = { path: 'a', unit: 'count', value: { available: 4 } };
    const gone: CounterReading = { path: 'a', unit: 'count', value: { unavailable: 'gone' } };
    expect([readingNumber(ok), readingReason(ok)]).toEqual([4, null]);
    expect([readingNumber(gone), readingReason(gone)]).toEqual([null, 'gone']);
    expect([readingNumber(undefined), readingReason(undefined)]).toEqual([null, null]);
  });
});

describe('lists', () => {
  it('adds once, respects the limit and never mutates', () => {
    const a = ['x'];
    expect(withPath(a, 'y', 3)).toEqual(['x', 'y']);
    expect(withPath(a, 'x', 3)).toEqual(['x']);
    expect(withPath(['x', 'y'], 'z', 2)).toEqual(['x', 'y']);
    expect(a).toEqual(['x']);
    expect(withoutPath(['x', 'y'], 'x')).toEqual(['y']);
  });

  it('collects the counters dashboard widgets show, once each', () => {
    expect(widgetCounterPaths([W('cpuTotal'), W({ perfCounter: '\\A\\b' }), W({ process: 'x' }), W({ perfCounter: '\\A\\b' })])).toEqual(['\\A\\b']);
  });
});

describe('the chart', () => {
  const history = (units: ('percent' | 'bytes' | 'count')[]): CounterHistory => ({
    from: 0,
    to: 60_000,
    series: units.map((unit, i) => ({ path: `\\O\\c${i}`, unit, points: [{ at: 1000, value: 1 }, { at: 2000, value: 2 }] })),
    events: [{ at: 1500, kind: 'cleanup', label: 'Cleanup ran' }],
  });

  it('draws one axis per unit, two at most, and says what it left out', () => {
    const one = counterChartOption(history(['percent', 'percent']), LIGHT);
    expect(one.units).toEqual(['percent']);
    expect(one.skipped).toEqual([]);
    const three = counterChartOption(history(['percent', 'bytes', 'count']), LIGHT);
    expect(three.units).toEqual(['percent', 'bytes']);
    expect(three.skipped).toEqual(['\\O\\c2']);
    const o = three.option as { series: { name: string; yAxisIndex: number }[]; yAxis: unknown[] };
    expect(o.series.map((s) => s.name)).toEqual(['c0', 'c1', 'Events']);
    expect(o.series[1]!.yAxisIndex).toBe(1);
    expect(o.yAxis).toHaveLength(3);
  });

  it('marks the events on their own lane and empties hidden lines', () => {
    const c = counterChartOption(history(['percent']), LIGHT, new Set(['\\O\\c0']));
    const series = (c.option as { series: { data: unknown[]; name: string }[] }).series;
    expect(series[0]!.data).toEqual([]);
    expect(series[1]!.name).toBe('Events');
    expect(series[1]!.data).toHaveLength(1);
  });

  it('formats the tooltip with the unit and the clock', () => {
    const c = counterChartOption(history(['bytes']), LIGHT);
    const tooltip = (c.option as { tooltip: { formatter: (p: unknown) => string } }).tooltip;
    const html = tooltip.formatter([{ axisValue: 1000, marker: '*', seriesName: 'c0', seriesType: 'line', value: [1000, 2000], data: { unit: 'bytes' } }]);
    expect(html).toContain('2 KB');
    expect(tooltip.formatter([])).toBe('');
  });
});

describe('byte counters follow their path', () => {
  it('uses binary steps for memory counters and decimal steps for disk and network counters', () => {
    expect(counterByteKind('\\Memory\\Available Bytes')).toBe('memory');
    expect(counterByteKind('\\Process(app)\\Working Set')).toBe('memory');
    expect(counterByteKind('\\Network Interface(en0)\\Bytes Sent/sec')).toBe('storage');
    expect(formatCounter(16 * 1024 ** 3, 'bytes', '\\Memory\\Available Bytes')).toEqual(['16', 'GB']);
    expect(formatCounter(16 * 1024 ** 3, 'bytes', '\\Physical Disk(0)\\Free Bytes')).toEqual(['17', 'GB']);
  });
});
