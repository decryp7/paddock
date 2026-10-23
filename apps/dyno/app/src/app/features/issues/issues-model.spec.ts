import type { Issue, SourceStatus } from '../../core/api.types';
import { DARK } from '../../shared/tokens';
import { NO_FILTER, dayLabel, filterIssues, issueRows, learningText, readSources, scoreOption, stabilityTone, trendText, unavailableSources, whyText } from './issues-model';
import { hue, initial } from './issues-model';

const NOW = new Date(2026, 9, 22, 14, 30).getTime();
const issue = (hoursAgo: number, over: Partial<Issue> = {}): Issue => ({
  id: `i${hoursAgo}`,
  at: NOW - hoursAgo * 3_600_000,
  kind: 'appCrash',
  severity: 'warning',
  app: 'Mail',
  title: 'Mail quit unexpectedly',
  evidence: 'EXC_BAD_ACCESS',
  source: 'macReports',
  ...over,
});

describe('issues model', () => {
  it('filters by text, kind and app', () => {
    const list = [issue(1), issue(2, { app: 'Slack', kind: 'appHang', evidence: 'spin' })];
    expect(filterIssues(list, NO_FILTER)).toHaveLength(2);
    expect(filterIssues(list, { ...NO_FILTER, search: 'SPIN' })).toHaveLength(1);
    expect(filterIssues(list, { ...NO_FILTER, kind: 'appHang' })[0]!.app).toBe('Slack');
    expect(filterIssues(list, { ...NO_FILTER, app: 'Mail' })).toHaveLength(1);
    expect(filterIssues(list, { ...NO_FILTER, search: 'app not responding' })).toHaveLength(1);
  });

  it('groups by local day with collapsible headings', () => {
    const list = [issue(1), issue(2), issue(30)];
    const rows = issueRows(list, NOW, new Set());
    expect(rows.map((r) => r.kind)).toEqual(['day', 'issue', 'issue', 'day', 'issue']);
    expect(rows[0]).toMatchObject({ label: 'Today', count: 2, open: true });
    expect(rows[3]).toMatchObject({ label: 'Yesterday', count: 1 });
    const collapsed = issueRows(list, NOW, new Set([rows[0]!.key]));
    expect(collapsed.map((r) => r.kind)).toEqual(['day', 'day', 'issue']);
    expect(collapsed[0]).toMatchObject({ open: false });
    expect(dayLabel(NOW - 5 * 86_400_000, NOW)).toMatch(/17/);
  });

  it('words the trend and the sources', () => {
    expect(trendText(0, 0, 14)).toBe('No issues in 14 days');
    expect(trendText(3, 3, 14)).toContain('the same as');
    expect(trendText(5, 2, 7)).toBe('5 issues in 7 days, 3 more than the 7 before');
    expect(trendText(1, 4, 7)).toBe('1 issue in 7 days, 3 fewer than the 7 before');
    const s: SourceStatus[] = [
      { source: 'macLog', label: 'System log', state: { unavailable: 'timed out' }, at: 1 },
      { source: 'macReports', label: 'Crash reports', state: { read: 2 }, at: 1 },
      { source: 'journal', label: 'Journal', state: 'pending', at: null },
    ];
    expect(unavailableSources(s)).toEqual([{ label: 'System log', reason: 'timed out' }]);
    expect(readSources(s)).toBe(1);
    expect(stabilityTone('unstable')).toBe('bad');
    expect(stabilityTone('someProblems')).toBe('warn');
    expect(stabilityTone('stable')).toBe('muted');
    expect(stabilityTone(null)).toBe('muted');
  });

  it('draws one bar per day with today highlighted and a tooltip', () => {
    const daily = [
      { day: NOW - 86_400_000, score: 100, issues: 0 },
      { day: NOW, score: 80, issues: 2 },
    ];
    const o = scoreOption(daily, DARK) as { series: { data: { value: number; itemStyle: { color: string } }[] }[]; tooltip: { formatter: (p: unknown) => string }; yAxis: { max: number } };
    expect(o.yAxis.max).toBe(100);
    expect(o.series[0]!.data.map((d) => d.value)).toEqual([100, 80]);
    expect(o.series[0]!.data[1]!.itemStyle.color).toBe(DARK.accent);
    expect(o.tooltip.formatter([{ dataIndex: 1 }])).toContain('Score 80, 2 issues');
    expect(o.tooltip.formatter([{ dataIndex: 0 }])).toContain('0 issues');
    expect(o.tooltip.formatter([])).toBe('');
    const long = scoreOption(Array.from({ length: 30 }, (_, i) => ({ day: i, score: 100, issues: 0 })), DARK) as { xAxis: { axisLabel: { interval: number } } };
    expect(long.xAxis.axisLabel.interval).toBe(1);
  });

  it('explains unusual programs with reasons, never verdicts', () => {
    expect(whyText({ level: 'unusual', reasons: ['runs from a temporary folder', 'was not seen on this computer before'] })).toBe('Unusual because it runs from a temporary folder');
    expect(whyText({ level: 'new', reasons: ['was not seen on this computer before'] })).toBe('New on this computer');
    expect(whyText({ level: 'new', reasons: ['is not signed by any publisher'] })).toBe('New on this computer; it is not signed by any publisher');
    expect(whyText({ level: 'unusual', reasons: ['was not seen on this computer before'] })).toBe('New on this computer');
  });

  it('avatars and learning text', () => {
    expect(initial(' zoom')).toBe('Z');
    expect(initial('')).toBe('?');
    expect(hue('a')).toBe(hue('a'));
    expect(hue('abc')).toBeLessThan(360);
    expect(learningText({ active: true, until: null }, 0, NOW)).toContain('about to start');
    expect(learningText({ active: true, until: NOW + 86_400_000 }, 0, NOW)).toContain('Learning what is normal here until');
    expect(learningText({ active: false, until: NOW }, 1, NOW)).toBe('Watching 1 known program');
    expect(learningText({ active: false, until: NOW }, 1234, NOW)).toBe('Watching 1,234 known programs');
  });
});
