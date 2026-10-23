import type { BootRow, Change, SoftwareItem } from '../../core/api.types';
import { LIGHT } from '../../shared/tokens';
import {
  CHANGE_WINDOWS,
  EXTRAS_TABS,
  bootOption,
  bootSecs,
  changeRows,
  changeTone,
  filterSoftware,
  heldForText,
  nextSort,
  sortUsage,
  wakeTimeText,
  watchingNote,
} from './extras-model';

const NOW = new Date(2026, 9, 22, 14, 30).getTime();
const H = 3_600_000;
const change = (ago: number, title: string): Change => ({ at: NOW - ago, kind: 'installed', title, detail: '' });

describe('extras model', () => {
  it('lists the eight tabs in order and three windows', () => {
    expect(EXTRAS_TABS.map((t) => t.label)).toEqual(['What changed', 'Sleep', 'Boot', 'Apps', 'GPU', 'Files', 'Inventory', 'Maintenance']);
    expect(CHANGE_WINDOWS.map((w) => w.hours)).toEqual([24, 72, 168]);
  });

  it('groups changes by local day, newest first, with a count per day', () => {
    const rows = changeRows([change(1 * H, 'a'), change(2 * H, 'b'), change(26 * H, 'c'), change(80 * H, 'd')], NOW);
    expect(rows.map((r) => (r.kind === 'day' ? `${r.label}:${r.count}` : r.change.title))).toEqual(['Today:2', 'a', 'b', 'Yesterday:1', 'c', expect.stringMatching(/:1$/), 'd']);
    expect(changeRows([], NOW)).toEqual([]);
  });

  it('colours only what is worth a look', () => {
    expect(changeTone('startupAdded')).toBe('warn');
    expect(changeTone('installed')).toBe('info');
    expect(changeTone('removed')).toBe('info');
    expect(changeTone('startupRemoved')).toBe('info');
    expect(changeTone('updated')).toBe('muted');
    expect(changeTone('setting')).toBe('muted');
  });

  it('says since when Dyno watches, and what it cannot know', () => {
    expect(watchingNote(null, NOW - 72 * H, NOW)).toContain('not recorded anything yet');
    expect(watchingNote(NOW - 9 * 24 * H, NOW - 72 * H, NOW)).toMatch(/^Dyno has been watching since .+\.$/);
    expect(watchingNote(NOW - 24 * H, NOW - 72 * H, NOW)).toContain('Changes before that are not known.');
  });

  it('reads the held-for and wake times the tools print', () => {
    expect(heldForText(null)).toBe('');
    expect(heldForText('00:42:17')).toBe('42 min 17 s');
    expect(heldForText('01:12:03')).toBe('1 h 12 min');
    expect(heldForText('a while')).toBe('a while');
    expect(wakeTimeText(null)).toBe('');
    expect(wakeTimeText('2026-10-22 08:01:12 +0800')).toMatch(/08:01$/);
    expect(wakeTimeText('yesterday evening')).toBe('yesterday evening');
  });

  it('formats boot seconds', () => {
    expect(bootSecs(47.2)).toBe('47.2 s');
    expect(bootSecs(null)).toBe('—');
    expect(bootSecs(72)).toBe('1 min 12 s');
  });

  it('draws boots oldest to newest, from 0, neutral with the latest in the accent colour', () => {
    const boots: BootRow[] = [
      { at: NOW, bootSecs: 40, readySecs: 60 },
      { at: NOW - 24 * H, bootSecs: null, readySecs: null },
      { at: NOW - 48 * H, bootSecs: 70, readySecs: 90 },
    ];
    const o = bootOption(boots, LIGHT) as { xAxis: { data: string[] }; yAxis: { min: number; name: string }; series: { data: { value: number; itemStyle: { color: string } }[] }[]; tooltip: { formatter: (p: unknown) => string } };
    expect(o.yAxis.min).toBe(0);
    expect(o.yAxis.name).toBe('seconds');
    expect(o.series[0]!.data.map((d) => d.value)).toEqual([70, 40]);
    expect(o.series[0]!.data[1]!.itemStyle.color).toBe(LIGHT.accent);
    expect(o.series[0]!.data[0]!.itemStyle.color).toBe(LIGHT.borderStrong);
    expect(o.xAxis.data).toHaveLength(2);
    expect(o.tooltip.formatter([{ dataIndex: 1 }])).toContain('40 s');
    expect(o.tooltip.formatter([{ dataIndex: 9 }])).toBe('');
    expect(o.tooltip.formatter([])).toBe('');
    expect(bootOption([], LIGHT)).toBeTruthy();
  });

  it('sorts usage by any column with missing numbers last, and flips on the same column', () => {
    const rows = [
      { app: 'b', bytesIn: 5, bytesOut: null, energyImpact: 2 },
      { app: 'a', bytesIn: 9, bytesOut: 1, energyImpact: null },
      { app: 'c', bytesIn: 5, bytesOut: null, energyImpact: 2 },
      { app: 'd', bytesIn: null, bytesOut: null, energyImpact: null },
    ];
    const order = (key: 'app' | 'bytesIn' | 'bytesOut' | 'energyImpact', dir: 'asc' | 'desc') => sortUsage(rows, { key, dir }).map((r) => r.app).join('');
    expect(order('bytesIn', 'desc')).toBe('abcd');
    expect(order('bytesIn', 'asc')).toBe('bcad');
    expect(order('bytesOut', 'desc')).toBe('abcd');
    expect(order('energyImpact', 'desc')).toBe('bcad');
    expect(order('app', 'asc')).toBe('abcd');
    expect(order('app', 'desc')).toBe('dcba');
    expect(nextSort({ key: 'app', dir: 'asc' }, 'app')).toEqual({ key: 'app', dir: 'desc' });
    expect(nextSort({ key: 'app', dir: 'desc' }, 'app')).toEqual({ key: 'app', dir: 'asc' });
    expect(nextSort({ key: 'app', dir: 'asc' }, 'bytesIn')).toEqual({ key: 'bytesIn', dir: 'desc' });
    expect(nextSort({ key: 'bytesIn', dir: 'desc' }, 'app')).toEqual({ key: 'app', dir: 'asc' });
  });

  it('filters software by name, publisher or version', () => {
    const items: SoftwareItem[] = [
      { name: 'Figma', version: '126.4', publisher: 'Figma, Inc.' },
      { name: 'Slack', version: null, publisher: null },
      { name: 'Xcode', version: '27.0', publisher: 'Apple' },
    ];
    expect(filterSoftware(items, '  ')).toHaveLength(3);
    expect(filterSoftware(items, 'apple').map((i) => i.name)).toEqual(['Xcode']);
    expect(filterSoftware(items, '126').map((i) => i.name)).toEqual(['Figma']);
    expect(filterSoftware(items, 'slack')).toHaveLength(1);
    expect(filterSoftware(items, 'zzz')).toEqual([]);
  });
});
