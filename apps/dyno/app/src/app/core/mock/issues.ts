import { ISSUE_KINDS } from '../../shared/labels';
import type { Severity, DayScore, Issue, IssueKind, IssuesOverview, IssuesQuery, SourceStatus, Stability, UnusualItem, UnusualOverview } from '../api.types';

const MIN = 60_000;
const HOUR = 60 * MIN;
const DAY = 24 * HOUR;

/** Points each kind costs a day, as the engine's `IssueKind::weight` (the mock only needs plausible numbers). */
const WEIGHT: Record<IssueKind, number> = {
  badShutdown: 20,
  memoryError: 20,
  diskError: 15,
  overheating: 10,
  memoryExhaustion: 10,
  appCrash: 8,
  driverFailure: 8,
  appHang: 5,
  serviceFailure: 5,
  updateFailure: 3,
};

const SEVERITY: Record<IssueKind, Severity> = {
  badShutdown: 'critical',
  diskError: 'critical',
  memoryError: 'critical',
  overheating: 'critical',
  appCrash: 'warning',
  appHang: 'warning',
  memoryExhaustion: 'warning',
  driverFailure: 'warning',
  serviceFailure: 'info',
  updateFailure: 'info',
};

interface Seed {
  ago: number;
  kind: IssueKind;
  app: string;
  title: string;
  evidence: string;
  source?: Issue['source'];
}

const SEEDS: Seed[] = [
  { ago: 35 * MIN, kind: 'appCrash', app: 'Google Chrome Helper (Renderer)', title: 'Google Chrome Helper (Renderer) quit unexpectedly', evidence: 'Version 156.0.8078.48. Exception EXC_CRASH (SIGABRT). Abort trap: 6. macOS 27.0.1 (26A434)' },
  { ago: 3 * HOUR, kind: 'appHang', app: 'Safari', title: 'Safari stopped responding', evidence: 'Report: /Users/alex/Library/Logs/DiagnosticReports/Safari_2026-10-22-101112_host.hang' },
  { ago: 9 * HOUR + 20 * MIN, kind: 'appCrash', app: 'Slack', title: 'Slack quit unexpectedly', evidence: 'Version 4.41.105. Exception EXC_BAD_ACCESS (SIGSEGV). macOS 27.0.1 (26A434)' },
  { ago: DAY + 5 * HOUR, kind: 'badShutdown', app: '', title: 'The Mac shut down without warning (cause 0)', evidence: 'AppleSMC: Previous shutdown cause: 0', source: 'macLog' },
  { ago: DAY + 5 * HOUR + 2 * MIN, kind: 'appCrash', app: 'Mail', title: 'Mail quit unexpectedly', evidence: 'Exception EXC_CRASH (SIGKILL). Launch Constraint Violation. macOS 27.0.1 (26A434)' },
  { ago: 2 * DAY + 2 * HOUR, kind: 'serviceFailure', app: 'com.logitech.manager.daemon', title: 'com.logitech.manager.daemon stopped with an error', evidence: 'com.logitech.manager.daemon[412]: Service exited with abnormal code: 78', source: 'macLog' },
  { ago: 3 * DAY + 7 * HOUR, kind: 'appCrash', app: 'Mail', title: 'Mail quit unexpectedly', evidence: 'Exception EXC_BAD_ACCESS (SIGSEGV). macOS 27.0.1 (26A434)' },
  { ago: 3 * DAY + 7 * HOUR + 10 * MIN, kind: 'appCrash', app: 'Mail', title: 'Mail quit unexpectedly', evidence: 'Exception EXC_BAD_ACCESS (SIGSEGV). macOS 27.0.1 (26A434)' },
  { ago: 4 * DAY + HOUR, kind: 'memoryExhaustion', app: 'Photoshop', title: 'The Mac ran out of memory and ended a program', evidence: 'low swap: killing largest compressed process with pid 8123 (Photoshop)', source: 'macLog' },
  { ago: 6 * DAY + 4 * HOUR, kind: 'appCrash', app: 'com.apple.WebKit.Networking', title: 'com.apple.WebKit.Networking quit unexpectedly', evidence: 'Version 627+. Exception EXC_GUARD. macOS 27.0.1 (26A434)' },
  { ago: 8 * DAY + 3 * HOUR, kind: 'appHang', app: 'Xcode', title: 'Xcode stopped responding', evidence: 'Report: Xcode_2026-10-15-091500_host.spin' },
  { ago: 10 * DAY + 6 * HOUR, kind: 'appCrash', app: 'Slack', title: 'Slack quit unexpectedly', evidence: 'Version 4.41.98. Exception EXC_BAD_ACCESS (SIGSEGV). macOS 27.0.1 (26A434)' },
  { ago: 12 * DAY + 2 * HOUR, kind: 'diskError', app: '', title: 'A disk reported an input/output error', evidence: 'disk3s1: I/O error.', source: 'macLog' },
];

const hash = (s: string): string => {
  let h = 0x811c9dc5;
  for (let i = 0; i < s.length; i++) h = Math.imul(h ^ s.charCodeAt(i), 0x01000193) >>> 0;
  return h.toString(16).padStart(16, '0');
};

/** The mock's issues for a clock reading, newest first. */
export function mockIssues(now: number): Issue[] {
  return SEEDS.map((s) => ({
    id: hash(`${s.kind}|${s.app}|${s.ago}`),
    at: now - s.ago,
    kind: s.kind,
    severity: SEVERITY[s.kind],
    app: s.app,
    title: s.title,
    evidence: s.evidence,
    source: s.source ?? 'macReports',
  })).sort((a, b) => b.at - a.at);
}

/** A day's score from its issues (the mock's stand-in for `issues::day_score`). */
function dayScore(list: Issue[]): number {
  const byKind = new Map<IssueKind, number>();
  for (const i of list) byKind.set(i.kind, (byKind.get(i.kind) ?? 0) + 1);
  let doubled = 0;
  for (const [k, n] of byKind) doubled += 2 * WEIGHT[k] + WEIGHT[k] * Math.min(n - 1, 4);
  return Math.max(0, 100 - Math.floor(doubled / 2));
}

const rating = (s: number): Stability => (s >= 90 ? 'stable' : s >= 70 ? 'someProblems' : 'unstable');

/** The Issues page for `query`, the way the engine builds it (local days from the clock's own zone). */
export function mockIssuesOverview(query: IssuesQuery, now: number, sources: SourceStatus[]): IssuesOverview {
  const days = Math.min(90, Math.max(1, Math.trunc(query.days)));
  const dayStart = (t: number) => {
    const d = new Date(t);
    return new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
  };
  const today = dayStart(now);
  const from = today - (days - 1) * DAY;
  const all = mockIssues(now);
  const window = all.filter((i) => i.at >= from && i.at <= now);
  const previous = all.filter((i) => i.at >= from - days * DAY && i.at < from).length;
  const daily: DayScore[] = Array.from({ length: days }, (_, n) => {
    const start = today - (days - 1 - n) * DAY;
    const list = window.filter((i) => i.at >= start && i.at < start + DAY);
    return { day: start, score: dayScore(list), issues: list.length };
  });
  const byApp = new Map<string, Issue[]>();
  for (const i of window.filter((x) => x.app)) byApp.set(i.app, [...(byApp.get(i.app) ?? []), i]);
  const apps = [...byApp.entries()]
    .map(([name, list]) => {
      const counts = new Map<IssueKind, number>();
      for (const i of list) counts.set(i.kind, (counts.get(i.kind) ?? 0) + 1);
      const kind = [...counts.entries()].sort((a, b) => b[1] - a[1])[0]![0];
      return { name, count: list.length, last: Math.max(...list.map((i) => i.at)), kind };
    })
    .sort((a, b) => b.count - a.count || b.last - a.last)
    .slice(0, 8);
  const kinds = ISSUE_KINDS
    .map((kind) => ({ kind, count: window.filter((i) => i.kind === kind).length }))
    .filter((k) => k.count > 0)
    .sort((a, b) => b.count - a.count);
  const readable = sources.some((s) => typeof s.state === 'object' && 'read' in s.state);
  const score = readable ? daily[daily.length - 1]!.score : null;
  return {
    from,
    to: now,
    days,
    score,
    rating: score === null ? null : rating(score),
    daily: readable ? daily : [],
    trend: { current: window.length, previous },
    apps,
    kinds,
    issues: window,
    total: window.length,
    sources,
    scannedAt: now - 4 * MIN,
  };
}

/** How the two Mac log sources read in the mock. */
export function mockSources(now: number): SourceStatus[] {
  return [
    { source: 'macReports', label: 'Crash reports', state: { read: 9 }, at: now - 4 * MIN },
    { source: 'macLog', label: 'System log', state: { read: 3 }, at: now - 4 * MIN },
  ];
}

const mk = (key: string, name: string, exe: string, ago: number, reasons: string[], score: number, over: Partial<UnusualItem> = {}): UnusualItem => ({
  key,
  name,
  exe,
  publisher: null,
  firstSeen: 0 - ago,
  lastSeen: 0,
  running: true,
  level: score >= 40 ? 'unusual' : 'new',
  score,
  status: 'new',
  reasons,
  ...over,
});

/** New and unusual programs: a few realistic cases, as offsets from the clock (resolved by `mockUnusual`). */
export function seedUnusual(): UnusualItem[] {
  return [
    mk('/private/var/folders/9k/x2/t/updater-helper', 'updater-helper', '/private/var/folders/9k/x2/T/updater-helper', 3 * HOUR, ['runs from a temporary folder', 'is not signed by any publisher', 'was not seen on this computer before'], 80),
    mk('/users/me/downloads/ngrok', 'ngrok', '/Users/me/Downloads/ngrok', DAY + 2 * HOUR, ['runs from the Downloads folder', 'was not seen on this computer before'], 55),
    mk('/library/application support/logi/logipluginservice', 'LogiPluginService', '/Library/Application Support/Logi/LogiPluginService', 5 * HOUR, ['was added to what starts at login', 'was not seen on this computer before'], 45, { running: false, lastSeen: 0 - 5 * HOUR + 10 * MIN }),
    mk('/applications/zoom.us.app/contents/macos/zoom.us', 'zoom.us', '/Applications/zoom.us.app/Contents/MacOS/zoom.us', 2 * DAY, ['was not seen on this computer before'], 15, { publisher: 'Zoom Video Communications, Inc.', running: false, lastSeen: 0 - 20 * HOUR }),
    mk('/users/me/library/application support/code/cachedextensions/rg', 'rg', '/Users/me/Library/Application Support/Code/CachedExtensions/rg', 4 * DAY, ['was not seen on this computer before'], 15, { publisher: 'Developer ID: Andrew Gallant', running: false, lastSeen: 0 - 3 * DAY }),
  ];
}

/** The unusual page for `items` at `now` (offsets in the seeds become times). */
export function mockUnusual(items: UnusualItem[], now: number, includeIgnored: boolean, extraIgnored: number): UnusualOverview {
  const shown = items
    .filter((i) => i.status === 'new' || (includeIgnored && (i.status === 'ignored' || i.status === 'protected')))
    .map((i) => ({ ...i, firstSeen: now + i.firstSeen, lastSeen: i.running ? now - MIN : now + i.lastSeen }))
    .sort((a, b) => Number(a.status !== 'new') - Number(b.status !== 'new') || b.score - a.score || b.firstSeen - a.firstSeen);
  return {
    at: now,
    enabled: true,
    learning: { active: false, until: now - 5 * DAY },
    learningDays: 7,
    known: 412,
    ignored: extraIgnored + items.filter((i) => i.status === 'ignored' || i.status === 'protected').length,
    items: shown,
  };
}
