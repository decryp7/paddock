import type {
  CleanupPlan,
  CleanupPolicy,
  CleanupReport,
  Config,
  DiskEntry,
  DiskScan,
  FinishedJob,
  HistoryEvent,
  JobInfo,
  MachineSample,
  Usage,
  UsagePoint,
  Mark,
  RunnerConfig,
  RunnerSnapshot,
  Span,
  UpdateOutcome,
  UpdatePolicy,
} from '../api.types';
import { DEFAULT_PROCESS_POLICY } from './mock-processes';

/*
 * Invented demo data for the browser build, UI tests and screenshots. Every name, repository and server here is made
 * up. None of this is the reference for behaviour: the Rust core decides statuses, summaries and plans; the mock only
 * looks plausible.
 */

export const MINUTE = 60_000;
export const HOUR = 60 * MINUTE;
export const DAY = 24 * HOUR;
export const GB = 1024 ** 3;
export const MB = 1024 ** 2;

/** A small deterministic PRNG (mulberry32), so the demo looks the same on every load. */
export function rng(seed: number): () => number {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

/** A stable pseudo-random value in [0, 1) for an integer key (for metrics that must not jump between requests). */
export function hashNoise(key: number): number {
  return rng(Math.floor(key) ^ 0x9e3779b9)();
}

/** Local midnight of the day containing `t`. */
export function startOfLocalDay(t: number): number {
  const d = new Date(t);
  d.setHours(0, 0, 0, 0);
  return d.getTime();
}

/** `2026-10-22T10:04:05+02:00` in local time, as logrus writes it. */
export function isoLocal(t: number): string {
  const d = new Date(t);
  const pad = (n: number) => String(Math.abs(n)).padStart(2, '0');
  const off = -d.getTimezoneOffset();
  const sign = off >= 0 ? '+' : '-';
  return (
    `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}` +
    `${sign}${pad(Math.trunc(off / 60))}:${pad(off % 60)}`
  );
}

export const SERVER = 'https://git.example.com';
export const FORGEJO_SERVER = 'https://code.example.org';
const DATA_DIR = 'C:\\ProgramData\\Pitcrew';

/** How one demo runner behaves over the generated history. */
export interface RunnerProfile {
  name: string;
  seed: number;
  jobsWeekday: number;
  jobsWeekend: number;
  avgJobMs: number;
  failRate: number;
  cancelRate: number;
  restartChance: number;
  repos: string[];
  jobs: string[];
  server: string;
  updates: { daysAgo: number; outcome: UpdateOutcome }[];
  /** Days ago with a longer outage (crash loop cool-down), in hours. */
  outages: { daysAgo: number; hour: number; hours: number }[];
  /** Paused for the last N hours (no jobs, no uptime). */
  pausedHours?: number;
}

export const PROFILES: RunnerProfile[] = [
  {
    name: 'build-win',
    seed: 11,
    jobsWeekday: 34,
    jobsWeekend: 4,
    avgJobMs: 9 * MINUTE,
    failRate: 0.11,
    cancelRate: 0.04,
    restartChance: 0.15,
    repos: ['acme/storefront', 'acme/payments-api', 'acme/design-system', 'tools/release-bot'],
    jobs: ['build (windows-latest)', 'test', 'lint', 'package', 'publish'],
    server: SERVER,
    updates: [
      { daysAgo: 19, outcome: { result: 'updated', plan: { from: '5.1.0', to: '5.1.1' } } },
      { daysAgo: 4, outcome: { result: 'updated', plan: { from: '5.1.1', to: '5.2.0' } } },
    ],
    outages: [],
  },
  {
    name: 'ui-tests',
    seed: 23,
    jobsWeekday: 18,
    jobsWeekend: 2,
    avgJobMs: 16 * MINUTE,
    failRate: 0.17,
    cancelRate: 0.06,
    restartChance: 0.25,
    repos: ['acme/storefront', 'acme/mobile-app', 'acme/design-system'],
    jobs: ['e2e (webview2)', 'visual regression', 'smoke'],
    server: SERVER,
    updates: [
      { daysAgo: 19, outcome: { result: 'updated', plan: { from: '5.1.0', to: '5.1.1' } } },
      { daysAgo: 11, outcome: { result: 'rolledBack', plan: { from: '5.1.1', to: '5.2.0' }, reason: 'notAcceptedInTime' } },
      { daysAgo: 10, outcome: { result: 'postponed', plan: { from: '5.1.1', to: '5.2.0' }, reason: 'busy' } },
      { daysAgo: 3, outcome: { result: 'updated', plan: { from: '5.1.1', to: '5.2.0' } } },
    ],
    outages: [],
  },
  {
    name: 'docker-lab',
    seed: 37,
    jobsWeekday: 12,
    jobsWeekend: 1,
    avgJobMs: 6 * MINUTE,
    failRate: 0.22,
    cancelRate: 0.03,
    restartChance: 0.45,
    repos: ['acme/payments-api', 'tools/base-images'],
    jobs: ['docker build', 'integration (postgres)', 'scan image'],
    server: SERVER,
    updates: [
      { daysAgo: 19, outcome: { result: 'updated', plan: { from: '5.1.0', to: '5.1.1' } } },
      {
        daysAgo: 4,
        outcome: { result: 'failed', plan: { from: '5.1.1', to: '5.2.0' }, reason: 'The checksums from dl.gitea.com and gitea.com differ.', needsAttention: false },
      },
    ],
    outages: [
      { daysAgo: 8, hour: 14, hours: 0.5 },
      { daysAgo: 0, hour: -1, hours: 0.5 },
    ],
  },
  {
    name: 'legacy-ci',
    seed: 53,
    jobsWeekday: 7,
    jobsWeekend: 0,
    avgJobMs: 22 * MINUTE,
    failRate: 0.08,
    cancelRate: 0.02,
    restartChance: 0.08,
    repos: ['legacy/erp-connector', 'legacy/report-engine'],
    jobs: ['msbuild', 'nunit', 'installer'],
    server: FORGEJO_SERVER,
    updates: [],
    outages: [],
    pausedHours: 26,
  },
];

/** The raw generated history of one runner. */
export interface RawHistory {
  jobs: (FinishedJob & { outcome: 'success' | 'failure' | 'cancelled'; queuedMs: number })[];
  up: Span[];
  marks: Mark[];
  events: HistoryEvent[];
  updates: { at: number; outcome: UpdateOutcome }[];
}

function jobInfo(p: RunnerProfile, r: () => number, n: number): JobInfo {
  const repo = p.repos[Math.floor(r() * p.repos.length)] ?? p.repos[0] ?? null;
  return {
    repository: repo,
    job: p.jobs[Math.floor(r() * p.jobs.length)] ?? null,
    jobId: 40_000 + n,
    runId: 9_000 + Math.floor(n / 3),
    runNumber: 100 + Math.floor(n / 3),
    taskId: 98_000 + n,
    server: p.server,
  };
}

/** What a generated job used at most, and what it left running (from a hash, so the main random stream is unchanged). */
function jobUse(n: number): Pick<FinishedJob, 'peakCpuPermille' | 'peakMemBytes' | 'leftovers'> {
  const k = hashNoise(n + 7_001);
  return { peakCpuPermille: Math.round(120 + k * 640), peakMemBytes: Math.round((0.4 + hashNoise(n + 9_007) * 5.5) * GB), leftovers: n % 7 === 3 ? 1 + (n % 3) : 0 };
}

/** 30 days (or `days`) of plausible history ending at `now`. */
export function generateHistory(p: RunnerProfile, now: number, days = 30): RawHistory {
  const r = rng(p.seed);
  const today = startOfLocalDay(now);
  const from = today - (days - 1) * DAY;
  const jobs: RawHistory['jobs'] = [];
  const downs: Span[] = [];
  const marks: Mark[] = [];
  const events: HistoryEvent[] = [];
  const updates: RawHistory['updates'] = [];
  const pausedFrom = p.pausedHours ? now - p.pausedHours * HOUR : Infinity;
  let n = 0;

  for (let d = 0; d < days; d++) {
    const dayStart = from + d * DAY;
    const weekday = new Date(dayStart).getDay();
    const weekend = weekday === 0 || weekday === 6;
    const base = weekend ? p.jobsWeekend : p.jobsWeekday;
    const count = Math.max(0, Math.round(base * (0.6 + r() * 0.8)));
    let cursor = dayStart + 7.5 * HOUR + r() * HOUR;
    for (let i = 0; i < count; i++) {
      cursor += (10.5 * HOUR) / Math.max(count, 1) * (0.4 + r() * 1.2);
      const duration = Math.max(40_000, p.avgJobMs * (0.35 + r() * 1.4));
      const startedAt = Math.round(cursor);
      const finishedAt = Math.round(startedAt + duration);
      if (finishedAt > now - 20 * MINUTE || startedAt > pausedFrom) break;
      const x = r();
      const outcome = x < p.failRate ? 'failure' : x < p.failRate + p.cancelRate ? 'cancelled' : 'success';
      const info = jobInfo(p, r, n++);
      const use = jobUse(n);
      jobs.push({ info, startedAt, finishedAt, ...use, outcome, queuedMs: Math.round(4_000 + r() * r() * 150_000) });
      events.push({ at: finishedAt, kind: 'jobFinished', job: { info, startedAt, finishedAt, ...use } });
      cursor = finishedAt;
    }
    // A nightly scheduled cleanup at 03:00.
    const cleanupAt = dayStart + 3 * HOUR + Math.round(r() * 10 * MINUTE);
    if (cleanupAt < now && cleanupAt < pausedFrom) {
      marks.push({ at: cleanupAt, kind: 'cleanup' });
      const failed = d % 6 === 2 ? 2 : 0;
      events.push({ at: cleanupAt, kind: 'cleanup', trigger: 'schedule', freedBytes: Math.round((4 + r() * 18) * GB), deleted: Math.round(300 + r() * 2000), failed });
    }
    // Occasional restarts: a stall (timed-out polls while idle) or a crash.
    if (r() < p.restartChance) {
      const at = dayStart + (9 + r() * 10) * HOUR;
      if (at < now - HOUR && at < pausedFrom) {
        const stall = r() < 0.55;
        marks.push({ at, kind: stall ? 'stall' : 'crash' });
        events.push(stall ? { at, kind: 'stalled', timeouts: 6 + Math.floor(r() * 3) } : { at, kind: 'exited', code: 1, expected: false });
        events.push({ at: at + 12_000, kind: 'started' });
        downs.push({ from: at, to: at + 12_000 });
      }
    }
  }

  for (const u of p.updates) {
    const at = today - u.daysAgo * DAY + 3 * HOUR + 20 * MINUTE;
    if (at < from) continue;
    updates.push({ at, outcome: u.outcome });
    const kind = u.outcome.result === 'updated' ? 'updated' : u.outcome.result === 'rolledBack' ? 'rolledBack' : u.outcome.result === 'failed' ? 'updateFailed' : 'updateSkipped';
    marks.push({ at, kind });
    events.push({ at, kind: 'updateFinished', outcome: u.outcome });
    if (u.outcome.result === 'updated' || u.outcome.result === 'rolledBack') downs.push({ from: at - 90_000, to: at });
  }

  for (const o of p.outages) {
    const start = o.hour < 0 ? now - 40 * MINUTE : today - o.daysAgo * DAY + o.hour * HOUR;
    const end = Math.min(now, start + o.hours * HOUR);
    if (start < from) continue;
    marks.push({ at: start, kind: 'crashLoop' });
    events.push({ at: start - 6 * MINUTE, kind: 'exited', code: 1, expected: false });
    events.push({ at: start, kind: 'crashLoop', exits: 5 });
    downs.push({ from: start, to: end });
  }

  if (p.pausedHours) {
    events.push({ at: pausedFrom, kind: 'paused' });
    downs.push({ from: pausedFrom, to: now });
  }

  downs.sort((a, b) => a.from - b.from);
  const up: Span[] = [];
  let at = from;
  for (const d of downs) {
    if (d.from > at) up.push({ from: at, to: d.from });
    at = Math.max(at, d.to);
  }
  if (at < now) up.push({ from: at, to: now });

  marks.sort((a, b) => a.at - b.at);
  events.sort((a, b) => b.at - a.at);
  jobs.sort((a, b) => a.startedAt - b.startedAt);
  return { jobs, up, marks, events, updates };
}

/** Clips spans to `[from, to]`. */
export function clipSpans(spans: Span[], from: number, to: number): Span[] {
  return spans.filter((s) => s.to > from && s.from < to).map((s) => ({ from: Math.max(s.from, from), to: Math.min(s.to, to) }));
}

/** The demo machine's samples over the last `rangeHours`, with a daily rhythm. */
export function machineSamples(now: number, rangeHours: number): MachineSample[] {
  const step = rangeHours <= 24 ? 5 * MINUTE : rangeHours <= 168 ? 30 * MINUTE : 2 * HOUR;
  const memTotal = 32 * GB;
  const diskTotal = 953 * GB;
  const out: MachineSample[] = [];
  const end = Math.floor(now / step) * step;
  for (let t = end - rangeHours * HOUR; t <= end; t += step) {
    const d = new Date(t);
    const hour = d.getHours() + d.getMinutes() / 60;
    const weekend = d.getDay() === 0 || d.getDay() === 6;
    const work = hour >= 8 && hour <= 19 ? Math.sin(((hour - 8) / 11) * Math.PI) : 0;
    const noise = hashNoise(t / step);
    const cpu = Math.min(980, Math.round(60 + (weekend ? 0.25 : 1) * work * 420 + noise * 140));
    const sinceCleanup = (t - (startOfLocalDay(t) + 3 * HOUR) + DAY) % DAY;
    out.push({
      at: t,
      cpuPermille: cpu,
      cpuPeakPermille: Math.min(1000, cpu + Math.round(120 + noise * 380)),
      memUsedBytes: Math.round((11.5 + (weekend ? 0.3 : 1) * work * 9 + noise * 1.6) * GB),
      memTotalBytes: memTotal,
      diskFreeBytes: Math.round((214 - (sinceCleanup / DAY) * 38 + noise * 1.5) * GB),
      diskTotalBytes: diskTotal,
      runners: {
        cpuPermille: Math.round(cpu * 0.55),
        cpuPeakPermille: Math.min(1000, Math.round(cpu * 0.55) + Math.round(100 + noise * 300)),
        memBytes: Math.round((3.2 + (weekend ? 0.3 : 1) * work * 5.5 + noise * 0.8) * GB),
        memPeakBytes: Math.round((4.6 + (weekend ? 0.3 : 1) * work * 6.2 + noise * 0.8) * GB),
        processes: Math.round(3 + work * 14 + noise * 4),
      },
    });
  }
  return out;
}

/** One demo runner's CPU and memory over the range; `scale` sets how busy it is (1 = the busiest). */
export function usageSeries(name: string, now: number, rangeHours: number, scale: number): UsagePoint[] {
  const seed = [...name].reduce((a, c) => a + c.charCodeAt(0), 0);
  return machineSamples(now, rangeHours).map((m, i) => {
    const n = hashNoise(i + seed);
    const cpu = Math.round(m.runners.cpuPermille * scale * (0.6 + n * 0.8));
    return {
      at: m.at,
      cpuPermille: cpu,
      cpuPeakPermille: Math.min(1000, cpu + Math.round(60 + n * 200)),
      memBytes: Math.round(m.runners.memBytes * scale * (0.7 + n * 0.6)),
      memPeakBytes: Math.round(m.runners.memPeakBytes * scale * (0.8 + n * 0.6)),
      processes: Math.max(1, Math.round(m.runners.processes * scale)),
    };
  });
}

/** The use now of a demo runner. */
export function demoUsage(cpuPermille: number, memGb: number, processes: number): Usage {
  return { cpuPermille, cpuPeakPermille: Math.min(1000, cpuPermille + 90), memBytes: Math.round(memGb * GB), memPeakBytes: Math.round(memGb * 1.2 * GB), processes };
}

/** A runner folder's size over the range: it grows with jobs and drops at the nightly cleanup. */
export function folderSeries(name: string, now: number, rangeHours: number, baseGb: number): [number, number][] {
  const step = rangeHours <= 24 ? 15 * MINUTE : rangeHours <= 168 ? HOUR : 4 * HOUR;
  const seed = [...name].reduce((a, c) => a + c.charCodeAt(0), 0);
  const out: [number, number][] = [];
  const end = Math.floor(now / step) * step;
  for (let t = end - rangeHours * HOUR; t <= end; t += step) {
    const sinceCleanup = (t - (startOfLocalDay(t) + 3 * HOUR) + DAY) % DAY;
    const grow = (sinceCleanup / DAY) * baseGb * 0.45;
    out.push([t, Math.round((baseGb + grow + hashNoise(t / step + seed) * 0.6) * GB)]);
  }
  return out;
}

export const DEFAULT_UPDATE: UpdatePolicy = {
  mode: 'automatic',
  pinned: null,
  window: { days: ['mon', 'tue', 'wed', 'thu', 'fri'], start: '02:00', end: '05:00' },
  check_every_hours: 24,
  channel: 'stable',
  skipped: [],
  deadline_days: 30,
};

export const DEFAULT_CLEANUP: CleanupPolicy = {
  automatic: true,
  every_hours: 24,
  work_max_age_hours: 72,
  cache_cap_mb: 20_480,
  folder_cap_mb: 102_400,
  min_free_mb: 20_480,
  keep_previous: true,
};

/** `pitcrew.toml` with Pitcrew's defaults and the given runners. */
export function defaultConfig(runners: RunnerConfig[]): Config {
  return {
    schema_version: 1,
    defaults: {
      update: structuredClone(DEFAULT_UPDATE),
      update_timeouts: { drain_secs: 10_800, stop_secs: 120, confirm_secs: 300 },
      watchdog: { threshold: 6, window_secs: 900 },
      restart: {
        initial_delay_secs: 10,
        max_delay_secs: 300,
        reset_after_secs: 600,
        crash_loop_restarts: 5,
        crash_loop_window_secs: 600,
        crash_loop_cooldown_secs: 1800,
      },
      logs: { max_bytes: 10 * MB, keep: 5 },
      cleanup: structuredClone(DEFAULT_CLEANUP),
      processes: structuredClone(DEFAULT_PROCESS_POLICY),
      disk_guard: { enabled: true, drain_below_mb: 4096, resume_above_mb: 6144, reserve_mb: 1024 },
      pause: { mode: 'off', windows: [{ days: [], start: '22:00', end: '06:00' }], busy_percent: 40, quiet_minutes: 5 },
    },
    runners,
  };
}

/** The demo runners' configuration entries. */
export function demoRunnerConfigs(): RunnerConfig[] {
  return [
    { name: 'build-win', kind: 'gitea-runner', dir: `${DATA_DIR}\\runners\\build-win`, labels: ['windows', 'x64', 'msvc'], session: 'interactive', logged_off: 'wait', server_url: SERVER, enabled: true },
    {
      name: 'ui-tests',
      kind: 'gitea-runner',
      dir: `${DATA_DIR}\\runners\\ui-tests`,
      labels: ['windows', 'webview2', 'desktop'],
      session: 'interactive',
      logged_off: 'wait',
      server_url: SERVER,
      enabled: true,
      update: { ...structuredClone(DEFAULT_UPDATE), mode: 'notify-only' },
    },
    {
      name: 'docker-lab',
      kind: 'gitea-runner',
      dir: 'D:\\runners\\docker-lab',
      labels: ['linux-container', 'docker'],
      session: 'service',
      logged_off: 'wait',
      server_url: SERVER,
      enabled: true,
      watchdog: { threshold: 4, window_secs: 600 },
    },
    {
      name: 'legacy-ci',
      kind: 'forgejo-runner',
      dir: `${DATA_DIR}\\runners\\legacy-ci`,
      labels: ['windows', 'dotnet-framework'],
      session: 'interactive',
      logged_off: 'service-account',
      server_url: FORGEJO_SERVER,
      enabled: true,
      binary_path: 'C:\\Tools\\forgejo-runner\\forgejo-runner.exe',
      update: { ...structuredClone(DEFAULT_UPDATE), mode: 'manual' },
      cleanup: { ...structuredClone(DEFAULT_CLEANUP), work_max_age_hours: 168 },
    },
  ];
}

/** The demo runners' snapshots at `now`. */
export function demoSnapshots(now: number, configs: RunnerConfig[], defaults: Config['defaults']): RunnerSnapshot[] {
  const policy = (c: RunnerConfig) => structuredClone(c.update ?? defaults.update);
  const base = (c: RunnerConfig): RunnerSnapshot => ({
    name: c.name,
    kind: c.kind,
    status: 'idle',
    enabled: c.enabled,
    pending: null,
    session: { kind: 'userSession' },
    runningSince: now - 3 * DAY - 4 * HOUR,
    nextStartAt: null,
    restarts24h: 0,
    problem: null,
    currentJob: null,
    currentVersion: '5.2.0',
    latestVersion: '5.2.0',
    checkedAt: now - 2 * HOUR - 14 * MINUTE,
    updateTarget: null,
    lastUpdate: null,
    updatePhase: { phase: 'idle' },
    updatePolicy: policy(c),
    managedBinary: !c.binary_path,
    dir: c.dir,
    labels: [...c.labels],
    serverUrl: c.server_url ?? null,
    sessionMode: c.session,
    disk: { folderBytes: 24 * GB, freeBytes: 196 * GB, low: false },
    nextCleanupAt: startOfLocalDay(now) + DAY + 3 * HOUR,
    usage: demoUsage(4, 0.06, 1),
    leftovers: 0,
    hold: null,
    deadline: null,
    updateLoop: null,
  });
  const byName = new Map(configs.map((c) => [c.name, c]));
  const out: RunnerSnapshot[] = [];
  const build = byName.get('build-win');
  if (build) {
    out.push({
      ...base(build),
      status: 'busy',
      currentJob: {
        info: { repository: 'acme/storefront', job: 'build (windows-latest)', jobId: 48_112, runId: 9_342, runNumber: 342, taskId: 99_812, server: SERVER },
        startedAt: now - 7 * MINUTE - 32_000,
      },
      lastUpdate: { outcome: { result: 'updated', plan: { from: '5.1.1', to: '5.2.0' } }, finishedAt: startOfLocalDay(now) - 4 * DAY + 3 * HOUR + 20 * MINUTE },
      disk: { folderBytes: 38.4 * GB, freeBytes: 196 * GB, low: false },
      usage: demoUsage(310, 4.2, 23),
      leftovers: 3,
    });
  }
  const ui = byName.get('ui-tests');
  if (ui) {
    out.push({
      ...base(ui),
      runningSince: now - 2 * DAY - 21 * HOUR,
      restarts24h: 1,
      latestVersion: '5.3.0',
      updateTarget: '5.3.0',
      lastUpdate: { outcome: { result: 'updated', plan: { from: '5.1.1', to: '5.2.0' } }, finishedAt: startOfLocalDay(now) - 3 * DAY + 3 * HOUR + 20 * MINUTE },
      disk: { folderBytes: 17.9 * GB, freeBytes: 196 * GB, low: false },
      usage: demoUsage(8, 0.4, 3),
      deadline: { behindSince: now - 26 * DAY, dueAt: now + 4 * DAY, daysLeft: 4, state: 'soon' },
    });
  }
  const docker = byName.get('docker-lab');
  if (docker) {
    out.push({
      ...base(docker),
      status: 'crashLoop',
      session: { kind: 'notRunning' },
      runningSince: null,
      nextStartAt: now + 22 * MINUTE,
      restarts24h: 7,
      problem: 'Exited 5 times in 10 minutes (exit code 1). Last line: "cannot ping the docker daemon, is it running?". Restarts are paused for 30 minutes.',
      currentVersion: '5.1.1',
      latestVersion: '5.3.0',
      updateTarget: '5.3.0',
      lastUpdate: {
        outcome: { result: 'failed', plan: { from: '5.1.1', to: '5.2.0' }, reason: 'The checksums from dl.gitea.com and gitea.com differ.', needsAttention: false },
        finishedAt: startOfLocalDay(now) - 4 * DAY + 3 * HOUR + 20 * MINUTE,
      },
      disk: { folderBytes: 71.2 * GB, freeBytes: 18.6 * GB, low: true },
      nextCleanupAt: now + 4 * MINUTE,
      usage: null,
    });
  }
  const legacy = byName.get('legacy-ci');
  if (legacy) {
    out.push({
      ...base(legacy),
      status: 'paused',
      session: { kind: 'serviceAccount', fallback: true },
      runningSince: null,
      currentVersion: '12.4.0',
      latestVersion: null,
      checkedAt: null,
      disk: { folderBytes: 9.3 * GB, freeBytes: 196 * GB, low: false },
      usage: null,
    });
  }
  return out;
}

function entry(path: string, kind: DiskEntry['kind'], gb: number, ageHours: number, now: number): DiskEntry {
  return { path, kind, bytes: Math.round(gb * GB), modified: now - ageHours * HOUR };
}

/** A runner folder's scan with work folders, caches, binaries and rotated logs. */
export function diskScan(snapshot: RunnerSnapshot, now: number): DiskScan {
  const exe = snapshot.kind === 'forgejo-runner' ? 'forgejo-runner.exe' : 'gitea-runner.exe';
  const entries: DiskEntry[] = [
    entry('work\\acme\\storefront', 'work', 12.4, 2, now),
    entry('work\\acme\\payments-api', 'work', 6.1, 121, now),
    entry('work\\acme\\design-system', 'work', 3.2, 26, now),
    entry('work\\tools\\release-bot', 'work', 1.1, 220, now),
    entry('cache\\actcache', 'cache', 8.2, 5, now),
    entry('work\\_tool\\tool_cache', 'cache', 4.6, 30, now),
    entry(`bin\\${exe}.previous`, 'previous', 0.041, 96, now),
    entry('bin\\gitea-runner-5.3.0.download', 'download', 0.022, 50, now),
    entry('bin\\act_runner.exe', 'oldBinary', 0.038, 2400, now),
    entry('logs\\runner.log.1', { rotatedLog: { n: 1 } }, 0.0098, 20, now),
    entry('logs\\runner.log.2', { rotatedLog: { n: 2 } }, 0.0098, 44, now),
    entry('logs\\runner.log.3', { rotatedLog: { n: 3 } }, 0.0098, 70, now),
    entry('logs\\runner.log.4', { rotatedLog: { n: 4 } }, 0.0098, 95, now),
  ];
  const scale = (snapshot.disk?.folderBytes ?? 36 * GB) / entries.reduce((a, e) => a + e.bytes, 0);
  for (const e of entries) if (e.kind === 'work' || e.kind === 'cache') e.bytes = Math.round(e.bytes * scale);
  return {
    entries,
    folderBytes: entries.reduce((a, e) => a + e.bytes, 0),
    freeBytes: snapshot.disk?.freeBytes ?? null,
    truncated: false,
  };
}

/** What a manual cleanup of `scan` would delete (plausible, not the core's rules). */
export function cleanupPreview(scan: DiskScan): CleanupPlan {
  const deletions: CleanupPlan['deletions'] = [];
  for (const e of scan.entries) {
    const ageH = (Date.now() - e.modified) / HOUR;
    if (e.kind === 'download' || e.kind === 'oldBinary') deletions.push({ entry: e, reason: 'leftover' });
    else if (e.kind === 'work' && ageH > 72) deletions.push({ entry: e, reason: 'aged' });
    else if (typeof e.kind === 'object' && e.kind.rotatedLog.n >= 3) deletions.push({ entry: e, reason: 'oldLog' });
    else if (e.kind === 'cache' && e.path.includes('tool_cache')) deletions.push({ entry: e, reason: 'cacheCap' });
  }
  return { trigger: 'manual', deletions, freedBytes: deletions.reduce((a, d) => a + d.entry.bytes, 0), stillNeededBytes: 0 };
}

/** The last nightly cleanup, with two files Windows would not let go of. */
export function lastCleanup(now: number): CleanupReport {
  return {
    trigger: 'schedule',
    at: startOfLocalDay(now) + 3 * HOUR + 4 * MINUTE,
    freedBytes: Math.round(14.2 * GB),
    deleted: 1843,
    failures: [
      { path: 'work\\_tool\\tool_cache\\node\\22.11.0\\x64\\node.exe', reason: 'access is denied (a file is read-only, in use, or owned by another account)' },
      { path: 'work\\acme\\payments-api\\.git\\objects\\pack\\pack-3f9c2e.idx', reason: 'the file is in use by another process' },
    ],
  };
}

const STEPS = ['Set up job', 'Checkout', 'Restore cache', 'Install dependencies', 'Build', 'Run tests', 'Upload artifacts', 'Post checkout'];

/** Runner log lines in logrus' text format, newest last. */
export function logLines(snapshot: RunnerSnapshot, now: number, count: number): string[] {
  const r = rng(snapshot.name.length * 97 + 5);
  const name = snapshot.name;
  const version = snapshot.currentVersion ?? '5.2.0';
  const labels = snapshot.labels.join(' ');
  const out: string[] = [];
  const line = (t: number, level: string, msg: string) => out.push(`time="${isoLocal(t)}" level=${level} msg="${msg.replace(/"/g, '\\"')}"`);
  let t = now - count * 40_000;
  line(t, 'info', 'Starting runner daemon');
  line(t + 300, 'info', `runner: ${name}, with version: v${version}, with labels: [${labels}], declare successfully`);
  let task = 99_700;
  while (out.length < count - 2) {
    t += 20_000 + r() * 40_000;
    const x = r();
    if (x < 0.5) {
      line(t, 'debug', 'fetch task: no task available');
    } else if (x < 0.62 && snapshot.status === 'crashLoop') {
      line(t, 'error', 'cannot ping the docker daemon, is it running? Cannot connect to the Docker daemon at npipe:////./pipe/docker_engine');
    } else if (x < 0.7) {
      line(t, 'warn', 'failed to fetch task: deadline_exceeded: context deadline exceeded (fetching a task timed out)');
    } else {
      task += 1;
      line(t, 'info', `task ${task} repo is acme/storefront ${SERVER} https://github.com`);
      for (const step of STEPS) {
        t += 4_000 + r() * 30_000;
        line(t, 'info', `[ci/build] ⭐ Run ${step}`);
        if (step === 'Run tests' && r() < 0.2) line(t + 500, 'warn', '[ci/build] test "checkout retries a failed payment" took 41.3s (slow)');
      }
      const ok = r() > 0.15;
      line(t + 800, ok ? 'info' : 'error', `[ci/build] 🏁 Job ${ok ? 'succeeded' : 'failed'}`);
    }
  }
  return out;
}
