import type { RecorderSample } from '../../core/api.types';
import { recorderSamples } from '../../core/mock/alarms';
import { DARK } from '../../shared/tokens';
import { momentInRange, niceMax, overviewPoints, panelHeadings, peakIndex, rangeFromDrag, sampleReadout, timelineOption, timelineSummary } from './recorder-options';

const NOW = 1_700_000_000_000;
const samples = recorderSamples(NOW, 60);

describe('recorder options', () => {
  it('finds the busiest sample, the first on a tie, 0 when empty', () => {
    expect(peakIndex([])).toBe(0);
    const s = (cpuTotal: number) => ({ ...samples[0]!, cpuTotal });
    expect(peakIndex([s(10), s(50), s(50), s(20)])).toBe(1);
  });

  it('rounds axis maxima up to 1, 2, 5 or 10 times a power of ten', () => {
    expect([0, 0.4, 1.2, 2.1, 3, 7, 135, 1000].map(niceMax)).toEqual([1, 1, 2, 5, 5, 10, 200, 1000]);
  });

  it('reads a sample out with units', () => {
    const r = sampleReadout({ ...samples[0]!, cpuTotal: 71.234, memoryUsed: 13.8 * 1024 ** 3, memoryTotal: 24 * 1024 ** 3, diskReadBps: 120 * 1024 ** 2 } as RecorderSample);
    expect(r.cpu).toBe('71.2%');
    expect(r.memory).toBe('13.8 of 24 GB');
    expect(r.disk).toContain('read 120');
    expect(r.net).toContain('MB/s');
  });

  it('summarises for screen readers and handles no samples', () => {
    expect(timelineSummary(samples)).toContain('60 one-second samples');
    expect(timelineSummary(samples)).toContain('arrow keys');
    expect(timelineSummary([])).toBe('No samples.');
  });

  it('headings carry the picked moment, and blank when there is none', () => {
    const h = panelHeadings(samples[3]);
    expect(h.map((x) => x.title)).toContain('Memory used, GB');
    expect(h[0]!.value).toMatch(/%$/);
    expect(panelHeadings(undefined).every((x) => x.value === '')).toBe(true);
  });

  it('builds five panels on one shared category axis with the moment marked', () => {
    const o = timelineOption(samples, 7, DARK) as { grid: unknown[]; xAxis: { data: string[] }[]; series: { type: string; markLine?: { data: { xAxis: number }[] } }[]; tooltip: { formatter: (p: unknown) => string } };
    expect(o.grid).toHaveLength(5);
    expect(o.xAxis.every((x) => x.data.length === 60)).toBe(true);
    expect(o.series.filter((s) => s.type === 'heatmap')).toHaveLength(1);
    expect(o.series[0]!.markLine!.data[0]!.xAxis).toBe(7);
    expect(o.tooltip.formatter([{ dataIndex: 2 }])).toContain('CPU');
    expect(o.tooltip.formatter([{ dataIndex: 999 }])).toBe('');
    expect(o.tooltip.formatter([])).toBe('');
  });

  it('copes with an empty snapshot', () => {
    expect(() => timelineOption([], 0, DARK)).not.toThrow();
  });

  it('turns a drag on the strip into a clamped sample range, ignoring clicks', () => {
    expect(rangeFromDrag(0, 100, 100, 61)).toEqual({ from: 0, to: 60 });
    expect(rangeFromDrag(75, 25, 100, 61)).toEqual({ from: 15, to: 45 });
    expect(rangeFromDrag(-50, 400, 100, 61)).toEqual({ from: 0, to: 60 });
    expect(rangeFromDrag(40, 41, 100, 61)).toBeNull();
    expect(rangeFromDrag(0, 50, 0, 61)).toBeNull();
    expect(rangeFromDrag(0, 50, 100, 1)).toBeNull();
  });

  it('keeps the picked moment inside the range', () => {
    expect(momentInRange(5, null)).toBe(5);
    expect(momentInRange(5, { from: 10, to: 20 })).toBe(10);
    expect(momentInRange(50, { from: 10, to: 20 })).toBe(20);
    expect(momentInRange(15, { from: 10, to: 20 })).toBe(15);
  });

  it('draws the overview outline and zooms the axes only when a range is set', () => {
    expect(overviewPoints([])).toBe('');
    const pts = overviewPoints(samples).split(' ');
    expect(pts[0]!.startsWith('0.00,')).toBe(true);
    expect(pts.at(-1)!.startsWith('100,')).toBe(true);
    expect((timelineOption(samples, 0, DARK) as { dataZoom?: unknown }).dataZoom).toBeUndefined();
    const zoomed = timelineOption(samples, 0, DARK, { from: 5, to: 20 }) as { dataZoom: { startValue: number; endValue: number }[] };
    expect(zoomed.dataZoom[0]).toMatchObject({ startValue: 5, endValue: 20 });
  });
});
