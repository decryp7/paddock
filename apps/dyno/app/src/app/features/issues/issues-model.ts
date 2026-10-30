import type { EChartsCoreOption } from 'echarts/core';
import type { DayScore, Issue, IssueKind, SourceStatus, Stability, UnusualItem } from '../../core/api.types';
import { formatDate } from '../../shared/format';
import { ISSUE_KIND_LABEL } from '../../shared/labels';
import type { Tone } from '../../shared/labels';
import type { ThemeTokens } from '../../shared/tokens';

/** The engine's word for a rating. */
export const STABILITY_LABEL: Record<Stability, string> = {
  stable: 'Stable',
  someProblems: 'Some problems',
  unstable: 'Unstable',
};

/** A rating's colour: calm when stable, a tone only when it is not. */
export function stabilityTone(r: Stability | null): Tone {
  return r === 'unstable' ? 'bad' : r === 'someProblems' ? 'warn' : 'muted';
}

/** What a filter on the issue table can match. */
export interface IssueFilter {
  search: string;
  kind: IssueKind | null;
  app: string | null;
}

export const NO_FILTER: IssueFilter = { search: '', kind: null, app: null };

/** The issues that match the filter (a presentation filter over what the engine returned). */
export function filterIssues(issues: readonly Issue[], f: IssueFilter): Issue[] {
  const q = f.search.trim().toLowerCase();
  return issues.filter(
    (i) =>
      (f.kind === null || i.kind === f.kind) &&
      (f.app === null || i.app === f.app) &&
      (q === '' || `${i.app} ${i.title} ${i.evidence} ${ISSUE_KIND_LABEL[i.kind]}`.toLowerCase().includes(q)),
  );
}

/** A row of the table: a day heading or an issue. */
export type IssueRow = { kind: 'day'; key: string; label: string; count: number; open: boolean } | { kind: 'issue'; key: string; issue: Issue };

const dayKey = (at: number): string => {
  const d = new Date(at);
  return `${d.getFullYear()}-${d.getMonth() + 1}-${d.getDate()}`;
};

/** The local calendar day before the one containing `now` (not 24 hours back: a day is 23 or 25 hours when the clocks change). */
const previousDayKey = (now: number): string => {
  const d = new Date(now);
  return dayKey(new Date(d.getFullYear(), d.getMonth(), d.getDate() - 1, 12).getTime());
};

/** "Today", "Yesterday" or "Oct 20" for the local day containing `at`. */
export function dayLabel(at: number, now: number): string {
  if (dayKey(at) === dayKey(now)) return 'Today';
  if (dayKey(at) === previousDayKey(now)) return 'Yesterday';
  return formatDate(at, now);
}

/** Newest-first issues as heading rows per local day, with the issues of collapsed days left out. */
export function issueRows(issues: readonly Issue[], now: number, collapsed: ReadonlySet<string>): IssueRow[] {
  const rows: IssueRow[] = [];
  let current: string | null = null;
  const counts = new Map<string, number>();
  for (const i of issues) counts.set(dayKey(i.at), (counts.get(dayKey(i.at)) ?? 0) + 1);
  for (const i of issues) {
    const k = dayKey(i.at);
    if (k !== current) {
      current = k;
      rows.push({ kind: 'day', key: `day:${k}`, label: dayLabel(i.at, now), count: counts.get(k) ?? 0, open: !collapsed.has(`day:${k}`) });
    }
    if (!collapsed.has(`day:${k}`)) rows.push({ kind: 'issue', key: i.id, issue: i });
  }
  return rows;
}

/** How the window's trend reads: "3 more than the 14 days before" or "same as before", never a colour on its own. */
export function trendText(current: number, previous: number, days: number): string {
  if (current === 0 && previous === 0) return `No issues in ${days} days`;
  const noun = (n: number) => `${n} ${n === 1 ? 'issue' : 'issues'}`;
  if (current === previous) return `${noun(current)} in ${days} days, the same as the ${days} before`;
  const diff = Math.abs(current - previous);
  return `${noun(current)} in ${days} days, ${diff} ${current > previous ? 'more' : 'fewer'} than the ${days} before`;
}

/** The sources that could not be read, with the engine's reason. */
export function unavailableSources(sources: readonly SourceStatus[]): { label: string; reason: string }[] {
  return sources.flatMap((s) => (typeof s.state === 'object' && 'unavailable' in s.state ? [{ label: s.label, reason: s.state.unavailable }] : []));
}

/** How many sources were read. */
export function readSources(sources: readonly SourceStatus[]): number {
  return sources.filter((s) => typeof s.state === 'object' && 'read' in s.state).length;
}

/** The daily score chart: one bar per local day, 0 to 100, today in the accent colour and days that were not stable marked. */
export function scoreOption(daily: readonly DayScore[], t: ThemeTokens): EChartsCoreOption {
  const font = 'system-ui, -apple-system, "Segoe UI", sans-serif';
  const text = { color: t.text3, fontFamily: font, fontSize: 11 };
  const last = daily.length - 1;
  return {
    animation: false,
    grid: { left: 36, right: 8, top: 8, bottom: 22 },
    tooltip: {
      trigger: 'axis',
      axisPointer: { type: 'shadow' },
      backgroundColor: t.overlay,
      borderColor: t.border,
      textStyle: { color: t.text1, fontFamily: font, fontSize: 12 },
      formatter: (params: unknown) => {
        const p = (params as { dataIndex: number }[])[0];
        const d = p ? daily[p.dataIndex] : undefined;
        return d ? `${formatDate(d.day)}<br/>Score ${d.score}, ${d.issues === 1 ? '1 issue' : `${d.issues} issues`}` : '';
      },
    },
    xAxis: {
      type: 'category',
      data: daily.map((d) => String(new Date(d.day).getDate())),
      axisLine: { lineStyle: { color: t.border } },
      axisTick: { show: false },
      axisLabel: { ...text, interval: daily.length > 16 ? 1 : 0 },
    },
    yAxis: {
      type: 'value',
      min: 0,
      max: 100,
      interval: 50,
      axisLabel: text,
      splitLine: { lineStyle: { color: t.chartGrid } },
    },
    series: [
      {
        type: 'bar',
        barMaxWidth: 18,
        data: daily.map((d, i) => ({
          value: d.score,
          itemStyle: { color: i === last ? t.accent : d.score < 90 ? t.warn : t.borderStrong, borderRadius: [2, 2, 0, 0] },
        })),
      },
    ],
  };
}

/** "Unusual because it runs from a temporary folder, is not signed" or "New on this computer". */
export function whyText(item: Pick<UnusualItem, 'level' | 'reasons'>): string {
  const reasons = item.reasons.filter((r) => r !== 'was not seen on this computer before');
  if (item.level === 'unusual' && reasons.length > 0) return `Unusual because it ${reasons.join(', ')}`;
  if (reasons.length > 0) return `New on this computer; it ${reasons.join(', ')}`;
  return 'New on this computer';
}

/** The first letter for an avatar tile. */
export function initial(name: string): string {
  const c = name.trim().charAt(0);
  return c ? c.toUpperCase() : '?';
}

/** A stable hue (0 to 359) for a name, for the letter tile. */
export function hue(name: string): number {
  let h = 0;
  for (let i = 0; i < name.length; i++) h = (h * 31 + name.charCodeAt(i)) % 360;
  return h;
}

/** The learning line: "Learning until Oct 30" while learning, else how many programs are known. */
export function learningText(learning: { active: boolean; until: number | null }, known: number, now: number): string {
  if (learning.active) return learning.until === null ? 'Dyno is about to start learning what is normal here' : `Learning what is normal here until ${formatDate(learning.until, now)}`;
  return `Watching ${known.toLocaleString()} known ${known === 1 ? 'program' : 'programs'}`;
}
