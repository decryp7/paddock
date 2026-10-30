import type { EChartsCoreOption } from 'echarts/core';
import type { AppUsageRow, BootRow, Change, ChangeKind, Prevents, SoftwareItem } from '../../core/api.types';
import { formatDate, formatDayTime, formatDuration, formatSeconds } from '../../shared/format';
import type { ThemeTokens } from '../../shared/tokens';
import { dayLabel } from '../issues/issues-model';

/** The tabs of the Extras page, in order. */
export type ExtrasTab = 'changes' | 'sleep' | 'boot' | 'apps' | 'gpu' | 'files' | 'inventory' | 'maintenance';

export const EXTRAS_TABS: readonly { id: ExtrasTab; label: string }[] = [
  { id: 'changes', label: 'What changed' },
  { id: 'sleep', label: 'Sleep' },
  { id: 'boot', label: 'Boot' },
  { id: 'apps', label: 'Apps' },
  { id: 'gpu', label: 'GPU' },
  { id: 'files', label: 'Files' },
  { id: 'inventory', label: 'Inventory' },
  { id: 'maintenance', label: 'Maintenance' },
];

// ---------------------------------------------------------------------------------------------- what changed

/** The windows "What changed" offers, in hours (the engine accepts up to 30 days). */
export const CHANGE_WINDOWS: readonly { hours: number; label: string }[] = [
  { hours: 24, label: '24 hours' },
  { hours: 72, label: '3 days' },
  { hours: 168, label: '7 days' },
];

export const CHANGE_KIND_LABEL: Record<ChangeKind, string> = {
  installed: 'Installed',
  updated: 'Updated',
  removed: 'Removed',
  startupAdded: 'New at start-up',
  startupRemoved: 'Gone from start-up',
};

/** A kind's dot: calm for the routine kinds, a tone only for what is worth a look (a new start-up item). */
export function changeTone(kind: ChangeKind): 'info' | 'warn' | 'muted' {
  return kind === 'startupAdded' ? 'warn' : kind === 'installed' || kind === 'removed' || kind === 'startupRemoved' ? 'info' : 'muted';
}

/** A row of the timeline: a day heading or a change. */
export type ChangeRow = { kind: 'day'; key: string; label: string; count: number } | { kind: 'change'; key: string; change: Change };

const dayKey = (at: number): string => {
  const d = new Date(at);
  return `${d.getFullYear()}-${d.getMonth() + 1}-${d.getDate()}`;
};

/** Newest-first changes as one table: a heading row per local day, then that day's changes. */
export function changeRows(changes: readonly Change[], now: number): ChangeRow[] {
  const rows: ChangeRow[] = [];
  const counts = new Map<string, number>();
  for (const c of changes) counts.set(dayKey(c.at), (counts.get(dayKey(c.at)) ?? 0) + 1);
  let current: string | null = null;
  changes.forEach((c, i) => {
    const k = dayKey(c.at);
    if (k !== current) {
      current = k;
      rows.push({ kind: 'day', key: `day:${k}`, label: dayLabel(c.at, now), count: counts.get(k) ?? 0 });
    }
    rows.push({ kind: 'change', key: `c${i}`, change: c });
  });
  return rows;
}

/** "Dyno has been watching since Oct 14." and, when the window reaches back further, that earlier changes are not known. */
export function watchingNote(watchingSince: number | null, from: number, now: number): string {
  if (watchingSince === null) return 'Dyno has not recorded anything yet; installs and updates appear here from now on.';
  const since = `Dyno has been watching since ${formatDate(watchingSince, now)}.`;
  return watchingSince > from ? `${since} Changes before that are not known.` : since;
}

// ----------------------------------------------------------------------------------------------------- sleep

export const PREVENTS_LABEL: Record<Prevents, string> = { sleep: 'Sleep', displaySleep: 'The display turning off', other: 'Something else' };

/** "00:42:17" as "42 min" (the tool prints hours:minutes:seconds); anything else is shown as printed. */
export function heldForText(heldFor: string | null): string {
  if (!heldFor) return '';
  const m = /^(\d+):(\d{2}):(\d{2})$/.exec(heldFor);
  if (!m) return heldFor;
  return formatDuration(Number(m[1]) * 3600 + Number(m[2]) * 60 + Number(m[3]));
}

/** A wake time as the tool printed it ("2026-10-22 08:01:12 +0800") as "Oct 22, 08:01"; unparsable text is shown as is. */
export function wakeTimeText(when: string | null): string {
  if (!when) return '';
  const m = /^(\d{4})-(\d{2})-(\d{2})[ T](\d{2}):(\d{2})/.exec(when);
  if (!m) return when;
  const at = new Date(Number(m[1]), Number(m[2]) - 1, Number(m[3]), Number(m[4]), Number(m[5])).getTime();
  return `${formatDate(at)}, ${m[4]}:${m[5]}`;
}

// ----------------------------------------------------------------------------------------------------- boot

/** Seconds in a label: "47.2 s", "1 min 12 s". */
export const bootSecs = (secs: number | null): string => formatSeconds(secs);

/**
 * Seconds until the system was up for each recent boot, oldest on the left, 0 at the bottom, the latest boot in the accent
 * colour and every other bar neutral.
 */
export function bootOption(boots: readonly BootRow[], t: ThemeTokens): EChartsCoreOption {
  const font = 'system-ui, -apple-system, "Segoe UI", sans-serif';
  const text = { color: t.text3, fontFamily: font, fontSize: 11 };
  const rows = boots.filter((b) => b.bootSecs !== null).slice(0, 12).reverse();
  const last = rows.length - 1;
  return {
    animation: false,
    grid: { left: 44, right: 8, top: 16, bottom: 24 },
    tooltip: {
      trigger: 'axis',
      axisPointer: { type: 'shadow' },
      backgroundColor: t.overlay,
      borderColor: t.border,
      textStyle: { color: t.text1, fontFamily: font, fontSize: 12 },
      formatter: (params: unknown) => {
        const p = (params as { dataIndex: number }[])[0];
        const b = p ? rows[p.dataIndex] : undefined;
        return b ? `${formatDayTime(b.at)}<br/>Up after ${bootSecs(b.bootSecs)}` : '';
      },
    },
    xAxis: {
      type: 'category',
      data: rows.map((b) => formatDate(b.at)),
      axisLine: { lineStyle: { color: t.border } },
      axisTick: { show: false },
      axisLabel: { ...text, hideOverlap: true },
    },
    yAxis: {
      type: 'value',
      min: 0,
      name: 'seconds',
      nameTextStyle: { ...text, align: 'right' },
      axisLabel: text,
      splitLine: { lineStyle: { color: t.chartGrid } },
    },
    series: [
      {
        type: 'bar',
        barMaxWidth: 24,
        label: { show: true, position: 'top', color: t.text2, fontFamily: font, fontSize: 11, formatter: (p: { value: number }) => `${Math.round(p.value)}` },
        data: rows.map((b, i) => ({ value: b.bootSecs ?? 0, itemStyle: { color: i === last ? t.accent : t.borderStrong, borderRadius: [2, 2, 0, 0] } })),
      },
    ],
  };
}

// ----------------------------------------------------------------------------------------------------- apps

export type UsageKey = 'app' | 'bytesIn' | 'bytesOut' | 'energyImpact';
export interface UsageSort {
  key: UsageKey;
  dir: 'asc' | 'desc';
}

/** The rows ordered by a column; a missing number sorts last whichever way the order runs. */
export function sortUsage(rows: readonly AppUsageRow[], sort: UsageSort): AppUsageRow[] {
  const sign = sort.dir === 'asc' ? 1 : -1;
  return [...rows].sort((a, b) => {
    if (sort.key === 'app') return sign * a.app.localeCompare(b.app);
    const x = a[sort.key];
    const y = b[sort.key];
    if (x === null && y === null) return a.app.localeCompare(b.app);
    if (x === null) return 1;
    if (y === null) return -1;
    return x === y ? a.app.localeCompare(b.app) : sign * (x - y);
  });
}

/** A column header click: the same column flips the order, a new column starts with the biggest (names with A to Z). */
export function nextSort(cur: UsageSort, key: UsageKey): UsageSort {
  if (cur.key === key) return { key, dir: cur.dir === 'asc' ? 'desc' : 'asc' };
  return { key, dir: key === 'app' ? 'asc' : 'desc' };
}

// ------------------------------------------------------------------------------------------------ inventory

/** Software whose name, publisher or version contains the text. */
export function filterSoftware(items: readonly SoftwareItem[], search: string): SoftwareItem[] {
  const q = search.trim().toLowerCase();
  return q === '' ? [...items] : items.filter((i) => `${i.name} ${i.publisher ?? ''} ${i.version ?? ''}`.toLowerCase().includes(q));
}

/** The first letter for an avatar tile. */
export { initial, hue } from '../issues/issues-model';

/** A row with its place in the list, for engine lists that carry no id of their own (a table row needs a key). */
export type Numbered<T> = T & { readonly n: number };

/** The rows of `list`, each with its place. */
export function numbered<T>(list: readonly T[]): Numbered<T>[] {
  return list.map((row, n) => ({ ...row, n }));
}

/** A numbered row's key. */
export const numberKey = (row: { readonly n: number }): string => String(row.n);
