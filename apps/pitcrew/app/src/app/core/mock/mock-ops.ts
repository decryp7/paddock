import type {
  Advice,
  CacheInfo,
  CacheLimits,
  CachePlan,
  ChannelSettings,
  EngineReport,
  Insights,
  PruneKind,
  PrunePreview,
  PruneSelection,
  RepoInsight,
} from '../api.types';
import { DAY, GB, HOUR, MINUTE } from './mock-data';

const MB = 1000 * 1000;

/** A small deterministic noise in 0..1 so the demo charts look alive but never change between runs. */
function noise(n: number): number {
  const x = Math.sin(n * 12.9898) * 43758.5453;
  return x - Math.floor(x);
}

const REPOS: { repo: string; jobs: number; ok: number; trend: number }[] = [
  { repo: 'acme/storefront', jobs: 9, ok: 0.94, trend: 0.02 },
  { repo: 'acme/payments-api', jobs: 6, ok: 0.88, trend: -0.1 },
  { repo: 'acme/design-system', jobs: 4, ok: 0.97, trend: 0 },
  { repo: 'acme/mobile-app', jobs: 3, ok: 0.82, trend: 0.04 },
];

function localMidnight(now: number): number {
  const d = new Date(now);
  d.setHours(0, 0, 0, 0);
  return d.getTime();
}

/** Insights for the demo world over `days` days, ending at `now`. */
export function demoInsights(now: number, days: number): Insights {
  const first = localMidnight(now) - (days - 1) * DAY;
  const repos: RepoInsight[] = REPOS.map((r, ri) => {
    const rows = Array.from({ length: days }, (_, d) => {
      const jobs = Math.round(r.jobs * (0.6 + noise(d * 7 + ri) * 0.8));
      const p = Math.min(0.995, Math.max(0.4, r.ok + r.trend * (d / days - 0.5) * 2 + (noise(d * 3 + ri) - 0.5) * 0.12));
      const failure = Math.round(jobs * (1 - p));
      return { start: first + d * DAY, success: jobs - failure, failure };
    });
    const sum = (a: typeof rows, k: 'success' | 'failure') => a.reduce((s, x) => s + x[k], 0);
    const half = Math.floor(days / 2);
    const share = (a: typeof rows) => {
      const s = sum(a, 'success');
      const f = sum(a, 'failure');
      return s + f === 0 ? null : Math.round((s * 1000) / (s + f));
    };
    const success = sum(rows, 'success');
    const failure = sum(rows, 'failure');
    return {
      repo: r.repo,
      jobs: success + failure,
      success,
      failure,
      successPermille: share(rows),
      earlierPermille: share(rows.slice(0, half)),
      laterPermille: share(rows.slice(half)),
      avgQueuedMs: Math.round((20 + noise(ri + 3) * 150) * 1000),
      days: rows,
    };
  }).sort((a, b) => b.jobs - a.jobs);
  const total = repos.reduce((s, r) => s + r.jobs, 0);
  const advice: Advice[] = [
    {
      kind: 'addCapacity',
      title: 'Jobs wait for a runner: add a runner or raise capacity',
      detail:
        'Jobs waited 2 min 12 s on average and 7 min 40 s at the 95th percentile (212 jobs measured); build-win was busy 86% of the time. Register another runner, or raise `runner.capacity` in the busiest runner’s configuration if the machine has room (Runners > Configuration).',
    },
    {
      kind: 'successDropped',
      title: 'acme/payments-api: the success rate dropped',
      detail: 'It was 93% in the first half of the range and 82% in the second. Look at the newest failures; if they are flaky jobs (see Flaky jobs) a retry policy may help.',
    },
  ];
  return {
    rangeDays: days,
    jobs: total,
    jobsWithOutcome: total - 14,
    successPermille: 892,
    repos,
    slowest: [
      { repo: 'acme/mobile-app', job: 'build (macos-latest)', runs: 18, avgMs: 22 * MINUTE + 40_000, p95Ms: 31 * MINUTE, maxMs: 38 * MINUTE },
      { repo: 'acme/storefront', job: 'e2e (windows-latest)', runs: 41, avgMs: 14 * MINUTE + 5000, p95Ms: 19 * MINUTE, maxMs: 24 * MINUTE },
      { repo: 'acme/payments-api', job: 'integration', runs: 33, avgMs: 11 * MINUTE + 20_000, p95Ms: 16 * MINUTE, maxMs: 21 * MINUTE },
      { repo: 'acme/storefront', job: 'build (windows-latest)', runs: 52, avgMs: 8 * MINUTE + 50_000, p95Ms: 12 * MINUTE, maxMs: 15 * MINUTE },
      { repo: 'acme/design-system', job: 'visual-regression', runs: 27, avgMs: 6 * MINUTE + 30_000, p95Ms: 9 * MINUTE, maxMs: 11 * MINUTE },
    ],
    flaky: [
      { repo: 'acme/payments-api', job: 'integration', runs: 33, failures: 7, flips: 9 },
      { repo: 'acme/storefront', job: 'e2e (windows-latest)', runs: 41, failures: 5, flips: 6 },
      { repo: 'acme/mobile-app', job: 'ui-tests', runs: 12, failures: 3, flips: 4 },
    ],
    queue: { samples: 198, avgMs: 2 * MINUTE + 12_000, p95Ms: 7 * MINUTE + 40_000, maxMs: 19 * MINUTE },
    runners: [
      { runner: 'build-win', jobs: 118, busyPermille: 860 },
      { runner: 'ui-tests', jobs: 54, busyPermille: 410 },
      { runner: 'mac-mini', jobs: 40, busyPermille: 270 },
    ],
    advice,
  };
}

/** The container engine report of the demo machine. */
export function demoEngineReport(now: number): EngineReport {
  return {
    engine: 'docker',
    problem: null,
    usage: [
      { what: 'Images', total: 34, active: 6, bytes: 18.4 * GB, reclaimableBytes: 12.1 * GB },
      { what: 'Containers', total: 9, active: 1, bytes: 640 * MB, reclaimableBytes: 610 * MB },
      { what: 'Local Volumes', total: 14, active: 3, bytes: 3.2 * GB, reclaimableBytes: 2.4 * GB },
      { what: 'Build Cache', total: 212, active: 0, bytes: 9.6 * GB, reclaimableBytes: 9.6 * GB },
    ],
    folders: [
      { label: 'Action checkouts (act)', path: 'C:\\Users\\decryp7\\.cache\\act', exists: true, bytes: 2.7 * GB, entries: 61, truncated: false },
      { label: 'Cache server (default)', path: 'C:\\Users\\decryp7\\.cache\\actcache', exists: true, bytes: 6.8 * GB, entries: 143, truncated: false },
    ],
    at: now,
  };
}

const EXAMPLES: Record<PruneKind, string[]> = {
  stoppedContainers: ['e41c0a  agitated_ride  node:20', '7ba2d9  eager_meitner  postgres:16', '19f3c0  buildx_buildkit0  moby/buildkit'],
  danglingImages: ['<none>:<none> (a1f09c)', '<none>:<none> (5d77e2)', '<none>:<none> (c0de17)'],
  unusedImages: ['ghcr.io/acme/ci-base:2026-08 (b44a10)', 'node:18-bookworm (9e02aa)', 'mcr.microsoft.com/dotnet/sdk:8.0 (30f5d1)'],
  buildCache: [],
  volumes: ['pgdata_old', 'a3f1…9c2b (anonymous)', 'cache_pnpm_tmp'],
};

const SIZES: Record<PruneKind, [number, number]> = {
  stoppedContainers: [6, 610 * MB],
  danglingImages: [11, 3.1 * GB],
  unusedImages: [17, 8.6 * GB],
  buildCache: [212, 9.6 * GB],
  volumes: [8, 0],
};

export function demoPrunePreview(selection: PruneSelection): PrunePreview {
  const items = selection.kinds.map((kind) => ({
    kind,
    count: SIZES[kind][0],
    bytes: kind === 'unusedImages' && selection.olderThanDays > 0 ? Math.round(SIZES[kind][1] * 0.6) : SIZES[kind][1],
    examples: EXAMPLES[kind],
  }));
  return { items, totalBytes: items.reduce((s, i) => s + i.bytes, 0) };
}

const CACHE_ENTRIES = [
  ['actions-cache-npm-4f2a', 1.9 * GB, 2],
  ['actions-cache-gradle-91bc', 1.4 * GB, 5],
  ['actions-cache-cargo-registry-7e10', 1.1 * GB, 1],
  ['actions-cache-pnpm-store-0a3d', 0.9 * GB, 9],
  ['actions-cache-nuget-c8d2', 0.7 * GB, 21],
  ['actions-cache-yarn-55aa', 0.5 * GB, 34],
  ['actions-cache-go-build-2b9f', 0.3 * GB, 48],
] as const;

export function demoCache(now: number, dir: string): CacheInfo {
  const top = CACHE_ENTRIES.map(([name, bytes, age]) => ({ name, bytes, modified: now - age * DAY - 3 * HOUR }));
  const bytes = top.reduce((s, e) => s + e.bytes, 0);
  return {
    enabled: true,
    dir,
    defaultDir: false,
    exists: true,
    bytes,
    entries: 143,
    top,
    sizeLimitMb: 6144,
    overLimit: bytes > 6144 * MB,
    freeBytes: 196 * GB,
    truncated: false,
    problem: null,
  };
}

export function demoCachePlan(info: CacheInfo, limits: CacheLimits, now: number): CachePlan {
  const entries: CachePlan['entries'] = [];
  let total = info.top.reduce((s, e) => s + e.bytes, 0);
  const byAge = [...info.top].sort((a, b) => a.modified - b.modified);
  const rest = [];
  for (const e of byAge) {
    if (limits.maxAgeDays > 0 && now - e.modified >= limits.maxAgeDays * DAY) {
      entries.push([e, 'old']);
      total -= e.bytes;
    } else rest.push(e);
  }
  if (limits.maxMb > 0) {
    for (const e of rest) {
      if (total <= limits.maxMb * MB) break;
      entries.push([e, 'overLimit']);
      total -= e.bytes;
    }
  }
  return { entries, bytes: entries.reduce((s, [e]) => s + e.bytes, 0) };
}

export function emptyChannels(): ChannelSettings {
  return { channels: [] };
}
