import type { History } from '../../core/api.types';
import { MockBackend } from '../../core/mock-backend';
import { DARK } from '../../shared/tokens';
import { coreHeatmapOption, heatCells, nearestIndex, roundTick } from './cpu-options';

async function history(os: 'macos' | 'windows' = 'macos'): Promise<History> {
  return new MockBackend({ latencyMs: 0, now: () => 1_700_000_000_000, os, cpu: os === 'macos' ? 'hybrid' : 'uniform' }).history('hour', null);
}

describe('per-core heatmap', () => {
  it('lays rows out performance first, one cell per core and bucket', async () => {
    const h = await history();
    const kinds = Array.from({ length: 14 }, (_, i) => (i < 4 ? 'efficiency' : 'performance')) as ('efficiency' | 'performance')[];
    const { cells, labels, times } = heatCells(h, kinds);
    expect(labels.slice(0, 2)).toEqual(['P1', 'P2']);
    expect(labels).toHaveLength(14);
    expect(cells).toHaveLength(14 * h.corePoints.length);
    expect(times).toHaveLength(h.corePoints.length);
    // The first row is core index 4 (the first performance core).
    expect(cells[0]![2]).toBe(h.corePoints[0]!.cores[4]!.avg);
  });

  it('uses plain labels for a uniform CPU and builds a heatmap series with the events marked', async () => {
    const h = await history('windows');
    const o = coreHeatmapOption(h, [], DARK) as { series: { type: string; markLine: { data: unknown[] } }[]; yAxis: { data: string[] } };
    expect(o.yAxis.data[0]).toBe('1');
    expect(o.series[0]!.type).toBe('heatmap');
    expect(Array.isArray(o.series[0]!.markLine.data)).toBe(true);
  });

  it('finds the nearest bucket', () => {
    expect(nearestIndex([0, 10, 20], 14)).toBe(1);
    expect(nearestIndex([0, 10, 20], 99)).toBe(2);
    expect(nearestIndex([], 5)).toBe(0);
  });

  it('has no cells without per-core history', async () => {
    const h = { ...(await history()), corePoints: [] };
    expect(heatCells(h, []).cells).toEqual([]);
  });
});

describe('roundTick', () => {
  it('keeps round times only, by range', () => {
    expect(roundTick('hour', '14:30')).toBe(true);
    expect(roundTick('hour', '14:35')).toBe(false);
    expect(roundTick('day', '15:00')).toBe(true);
    expect(roundTick('day', '16:00')).toBe(false);
    expect(roundTick('day', '15:05')).toBe(false);
    expect(roundTick('week', '22 Oct 00:00')).toBe(true);
    expect(roundTick('week', '22 Oct 12:00')).toBe(false);
    expect(roundTick('month', 'nonsense')).toBe(false);
  });
});
