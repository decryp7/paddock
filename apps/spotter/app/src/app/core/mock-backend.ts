import type {
  Avatar,
  MailStatus,
  AppEvent,
  AppInfo,
  AppUpdateCheck,
  AppUpdateStatus,
  CheckRecord,
  CommandError,
  Dashboard,
  DashboardRange,
  DiscoveredFeed,
  DownloadBucket,
  ImportFormat,
  ImportResult,
  OpenTarget,
  PatternSelection,
  PatternTest,
  Product,
  Preset,
  Release,
  RendererStatus,
  SchemePreset,
  Schedule,
  SchedulePreview,
  Settings,
  SourceKind,
  SourcePreview,
  StatePush,
  VersionSchemeId,
  WatchDetail,
  WatchDraft,
  WatchNumbers,
  WatchStatus,
  WatchSummary,
} from './api.types';
import type { ReleaseTarget, SpotterBackend } from './backend';
import { blankDraft } from './draft';
import { DAY, HOUR, MINUTE, PRESETS, SOURCE_KINDS, makeReleases, rng } from './mock/mock-data';
import { mockDescribe, mockSchedulePreview, mockScheduleProblems } from './mock/mock-schedule';
import { MOCK_SCHEMES, mockExtraction, mockSuggest, mockTest, patternProblem } from './mock/mock-extraction';

export type MockScenario = 'demo' | 'empty' | 'broken' | 'products';

export interface MockOptions {
  scenario?: MockScenario;
  /** Milliseconds every call takes (0 in tests). */
  latencyMs?: number;
  now?: () => number;
  platform?: AppInfo['platform'];
}

interface MockWatch {
  spec: WatchDraft;
  releases: Release[];
  checks: CheckRecord[];
  ack: string | null;
  installed: string | null;
  status: WatchStatus;
  checking: boolean;
  lastCheckMs: number | null;
  failures: number;
  secrets: string[];
  /** Only in the 'products' scenario. */
  products?: Product[];
}

const OK: WatchStatus = { kind: 'ok', message: null };

function problem(code: CommandError['code'], message: string, problems: string[] = []): CommandError {
  return { code, message, problems };
}

/** A simple bundled-style mark for a source kind (the mock never fetches a picture). */
function mockMark(kind: string): Avatar | null {
  if (!kind) return null;
  const svg = '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#0E8577" stroke-width="1.8"><circle cx="12" cy="12" r="8"/></svg>';
  return { source: 'bundled', mime: 'image/svg+xml', dataUrl: 'data:image/svg+xml;base64,' + btoa(svg) };
}

/** The mock's seed watches: invented names, addresses and versions. */
function demoWatches(now: number): MockWatch[] {
  const mk = (
    seed: number,
    name: string,
    kind: string,
    values: Record<string, string>,
    scheme: VersionSchemeId,
    count: number,
    prefix: string,
    extra: Partial<MockWatch> = {},
    spec: Partial<WatchDraft> = {},
  ): MockWatch => {
    const releases = makeReleases(seed, scheme, count, now - (seed % 5) * 6 * HOUR - 40 * MINUTE, 330, prefix).reverse();
    const checkRand = rng(seed + 100);
    const checks: CheckRecord[] = Array.from({ length: 30 }, (_, i) => ({
      atMs: now - (30 - i) * 6 * HOUR,
      tookMs: Math.round(200 + checkRand() * 1400),
      error: seed % 3 === 1 && (i === 9 || i === 21) ? 'The server did not answer within 20 seconds.' : null,
      found: count - (i < 14 ? 2 : i < 25 ? 1 : 0),
    }));
    return {
      spec: blankDraft({ id: `w-${seed}`, name, source: { kind, values }, versions: scheme, ...spec }),
      releases,
      checks,
      ack: releases[1]?.version ?? null,
      installed: releases[3]?.version ?? null,
      status: OK,
      checking: false,
      lastCheckMs: now - 20 * MINUTE,
      failures: 0,
      secrets: [],
      ...extra,
    };
  };
  const acme = mk(1, 'Acme Installer', 'folder', { path: '/srv/drops/acme-installer' }, 'build-number', 36, 'AcmeSetup-');
  acme.ack = acme.releases[2]?.version ?? null;
  const lumen = mk(2, 'Lumen CLI', 'github', { repo: 'lumen-labs/lumen' }, 'semver', 28, 'lumen-');
  lumen.ack = lumen.releases[0]?.version ?? null;
  lumen.installed = lumen.releases[0]?.version ?? null;
  const fjord = mk(3, 'Fjord UI kit', 'gitea', { server: 'https://codeberg.example', repo: 'fjord/ui-kit' }, 'semver', 18, 'fjord-ui-');
  const orbit = mk(4, 'Orbit server', 'gitlab', { server: 'https://gitlab.example', project: 'orbit/server' }, 'semver', 22, 'orbit-', {
    status: { kind: 'unreachable', message: 'gitlab.example did not answer within 20 seconds. It will be tried again in 15 minutes.' },
    failures: 3,
    lastCheckMs: now - 11 * MINUTE,
  });
  const image = mk(5, 'Orbit API image', 'oci', { image: 'registry.example/orbit/api' }, 'semver', 32, 'api:', {
    status: { kind: 'authNeeded', message: 'The registry asked for a token. Add one in the watch settings.' },
    failures: 1,
  });
  const pkg = mk(6, 'tinyparse', 'npm', { package: 'tinyparse' }, 'semver', 14, 'tinyparse-');
  const vendor = mk(7, 'Vendor download page', 'web-page', { url: 'https://vendor.example/download', selector: 'h2', render: 'auto' }, 'numeric', 10, 'Vendor-', {
    status: { kind: 'needsYou', message: 'The page asks for a login before it shows the download. Open it, sign in, then check again.' },
  });
  const docs = mk(8, 'Nightly docs feed', 'feed', { url: 'https://docs.example/releases.atom' }, 'date', 12, 'docs-', {}, { paused: true });
  return [acme, lumen, fjord, orbit, image, pkg, vendor, docs];
}

/** A watch whose pattern has an `id` group: three products, each with its own releases and files (invented names). */
function productWatch(now: number): MockWatch {
  const file = (name: string, variant: string, at: number): Release['artifacts'][number] => ({
    name,
    location: `/srv/drops/${name}`,
    kind: name.split('.').pop() ?? '',
    variant,
    size: 600_000_000,
    modifiedMs: at,
    createdMs: null,
    checksumFile: variant === 'x64' ? `${name}.sha256` : null,
  });
  const rel = (product: string, version: string, ago: number, variants: string[]): Release => {
    const at = now - ago;
    const artifacts = variants.map((v) => file(`Acme-${product}-${version}${v ? '_' + v : ''}.${v ? 'exe' : 'iso'}`, v, at));
    return {
      version,
      prerelease: false,
      name: artifacts[0]!.name,
      location: artifacts[0]!.location,
      publishedAtMs: at,
      firstSeenMs: at + 5 * MINUTE,
      size: 600_000_000,
      sha256: null,
      product,
      channel: '',
      fields: {},
      artifacts,
      updatedInPlace: false,
    };
  };
  const pro = [rel('PRO', 'R3.10.00T32', 3 * HOUR, ['', 'x64']), rel('PRO', 'R3.09.00T40', 9 * DAY, [''])];
  const qa = [rel('QA', 'R1.01.00T07', 2 * DAY, [''])];
  const product = (id: string, name: string, history: Release[], unseen: boolean): Product => ({ id, name, muted: false, latest: history[0] ?? null, unseen, history });
  const products = [product('PRO', 'Acme Installer Pro', pro, true), product('QA', 'QA', qa, false)];
  return {
    spec: blankDraft({
      id: 'w-9',
      name: 'Acme Installer Suite',
      source: { kind: 'folder', values: { path: '/srv/drops/acme-suite', depth: '1' } },
      versions: 'custom',
      versionPattern: 'Acme-(?<id>[A-Z]+)-R(?<major>\\d+)\\.(?<minor>\\d+)\\.(?<patch>\\d+)T(?<build>\\d+)(?:_(?<variant>x64|arm64))?',
    }),
    releases: [...pro, ...qa].sort((a, b) => b.firstSeenMs - a.firstSeenMs),
    checks: [{ atMs: now - 20 * MINUTE, tookMs: 2400, error: null, found: 3 }],
    ack: null,
    installed: null,
    status: OK,
    checking: false,
    lastCheckMs: now - 20 * MINUTE,
    failures: 0,
    secrets: [],
    products,
  };
}

const DEFAULT_SETTINGS: Settings = {
  theme: 'system',
  notifications: true,
  notificationsPausedUntilMs: null,
  checkAppUpdates: true,
  downloadsFolder: null,
  defaultIntervalSecs: 6 * 3600,
  digest: 'off',
  quietHours: null,
  allowOsWebview: false,
  smtp: null,
};

/**
 * The browser build's backend and the tests': it mimics the contract with plausible, invented data and keeps its
 * state in memory. It is never the reference for behaviour (the Rust engine is).
 */
export class MockBackend implements SpotterBackend {
  private readonly now: () => number;
  private readonly latency: number;
  private readonly scenario: MockScenario;
  private readonly platform: AppInfo['platform'];
  private watches: MockWatch[];
  private settings: Settings = { ...DEFAULT_SETTINGS };
  private autostart = false;
  private mailPassword = false;
  private renderer: RendererStatus = { kind: 'none', detail: 'No Chromium-based browser was found. Install one, or download the pinned Chromium (about 150 MB).', canDownload: true };
  private updateCount = 0;
  private nextId = 100;
  private readonly stateHandlers = new Set<(p: StatePush) => void>();
  private readonly navigateHandlers = new Set<(r: string) => void>();
  /** Calls made, for tests. */
  readonly calls: string[] = [];

  constructor(options: MockOptions = {}) {
    this.now = options.now ?? Date.now;
    this.latency = options.latencyMs ?? 0;
    this.scenario = options.scenario ?? 'demo';
    this.platform = options.platform ?? 'macos';
    this.watches = this.scenario === 'empty' ? [] : this.scenario === 'products' ? [...demoWatches(this.now()), productWatch(this.now())] : demoWatches(this.now());
  }

  /** Test helper: delivers a pushed state to subscribers. */
  push(): void {
    const p = { watches: this.summaries() };
    this.stateHandlers.forEach((h) => h(p));
  }

  /** Test helper: asks subscribers to show a route. */
  navigate(route: string): void {
    this.navigateHandlers.forEach((h) => h(route));
  }

  // --- helpers -------------------------------------------------------------------------------------------------

  private async call<T>(name: string, work: () => T): Promise<T> {
    this.calls.push(name);
    if (this.latency > 0) await new Promise((r) => setTimeout(r, this.latency));
    if (this.scenario === 'broken') throw problem('unavailable', 'The engine is not answering (mock "broken" scenario).');
    return work();
  }

  private find(id: string): MockWatch {
    const w = this.watches.find((x) => x.spec.id === id);
    if (!w) throw problem('notFound', 'That watch no longer exists.');
    return w;
  }

  private latest(w: MockWatch): Release | null {
    return (w.spec.includePrereleases ? w.releases[0] : w.releases.find((r) => !r.prerelease)) ?? null;
  }

  private summary(w: MockWatch): WatchSummary {
    const latest = this.latest(w);
    const idx = (v: string | null) => (v === null ? -1 : w.releases.findIndex((r) => r.version === v));
    const latestIdx = latest ? idx(latest.version) : -1;
    const ackIdx = idx(w.ack);
    const instIdx = idx(w.installed);
    const kind = SOURCE_KINDS.find((k) => k.id === w.spec.source.kind);
    const v = w.spec.source.values;
    const description = v['repo'] ?? v['url'] ?? v['path'] ?? v['image'] ?? v['package'] ?? v['project'] ?? v['crate'] ?? '';
    return {
      id: w.spec.id ?? '',
      name: w.spec.name,
      sourceKind: w.spec.source.kind,
      sourceLabel: kind?.label ?? w.spec.source.kind,
      sourceDescription: description,
      status: w.status,
      paused: w.spec.paused,
      checking: w.checking,
      latest,
      acknowledged: w.ack,
      installed: w.installed,
      unseen: latest !== null && (w.ack === null || ackIdx === -1 || latestIdx < ackIdx),
      newerThanInstalled: w.installed === null || instIdx === -1 || latest === null ? null : latestIdx < instIdx,
      lastCheckMs: w.lastCheckMs,
      nextCheckMs: w.spec.paused || w.lastCheckMs === null ? null : (mockSchedulePreview(w.spec.schedule, w.lastCheckMs).nextRunsMs[0] ?? null),
      failures: w.failures,
      scheduleText: mockDescribe(w.spec.schedule),
      productCount: w.products?.length ?? 0,
    };
  }

  private release(id: string, version: string, target?: ReleaseTarget): Release {
    const w = this.find(id);
    const pool = target?.product ? (w.products?.find((p) => p.id === target.product)?.history ?? []) : w.releases;
    const r = pool.find((x) => x.version === version);
    if (!r) throw problem('notFound', 'That release is gone.');
    if (target?.artifact && !r.artifacts.some((a) => a.name === target.artifact)) throw problem('badRequest', `The release has no file called "${target.artifact}".`);
    return r;
  }

  private summaries(): WatchSummary[] {
    return this.watches.map((w) => this.summary(w));
  }

  private emit(): void {
    this.push();
  }

  private bucketStarts(range: DashboardRange): { starts: number[]; width: number } {
    const now = this.now();
    const [count, width] = range === '30d' ? [30, DAY] : range === '90d' ? [90, DAY] : [52, 7 * DAY];
    const start = Math.floor(now / width) * width;
    return { starts: Array.from({ length: count }, (_, i) => start - (count - 1 - i) * width), width };
  }

  // --- SpotterBackend --------------------------------------------------------------------------------------------

  appInfo(): Promise<AppInfo> {
    return this.call('appInfo', () => ({
      version: '0.1.0',
      configPath: '/home/demo/.config/spotter-demo/config.toml',
      logsDir: '/home/demo/.local/share/spotter-demo/logs',
      downloadsDir: '/home/demo/Downloads/Spotter-demo',
      warnings: this.scenario === 'demo' ? ['The watch "Old mirror" had an unknown source kind and was left out.'] : [],
      migratedFromLegacy: this.scenario === 'demo',
      platform: this.platform,
    }));
  }

  listWatches(): Promise<WatchSummary[]> {
    return this.call('listWatches', () => this.summaries());
  }

  getWatch(id: string): Promise<WatchDetail> {
    return this.call('getWatch', () => {
      const w = this.find(id);
      return { summary: this.summary(w), spec: structuredClone(w.spec), history: w.releases, checks: w.checks, secretsSet: w.secrets, products: structuredClone(w.products ?? []) };
    });
  }

  sourceKinds(): Promise<SourceKind[]> {
    return this.call('sourceKinds', () => SOURCE_KINDS);
  }

  testSource(draft: WatchDraft, secrets: Record<string, string>): Promise<SourcePreview> {
    return this.call('testSource', () => {
      void secrets;
      const kind = SOURCE_KINDS.find((k) => k.id === draft.source.kind);
      const problems: string[] = [];
      if (!kind) problems.push(`Unknown source kind "${draft.source.kind}".`);
      for (const f of kind?.fields ?? []) {
        if (f.required && f.kind !== 'secret' && !(draft.source.values[f.key] ?? '').trim()) problems.push(`${f.label} is required.`);
      }
      if (draft.versions === 'custom' && !(draft.versionPattern ?? '').trim()) problems.push('A custom version scheme needs a pattern.');
      if (draft.versions === 'custom' && draft.versionPattern?.trim()) {
        const bad = patternProblem(draft.versionPattern);
        if (bad) problems.push(bad);
      }
      const empty: SourcePreview = { problems, error: null, releases: [], withoutVersion: [], prereleasesSkipped: 0, namesSkipped: 0, tookMs: 0, extraction: null };
      if (problems.length) return empty;
      if (draft.source.kind === 'folder') return { ...empty, extraction: mockExtraction(draft, this.now()), tookMs: 2400 };
      const target = Object.values(draft.source.values).join(' ');
      if (/unreachable|offline/i.test(target)) {
        return { ...empty, error: { kind: 'unreachable', message: 'The server did not answer. Check the address and your connection.' }, tookMs: 20_000 };
      }
      const all = makeReleases(target.length * 7 + 3, draft.versions, 14, this.now(), 200, '').reverse();
      let prereleasesSkipped = 0;
      let namesSkipped = 0;
      const releases = all.filter((r) => {
        if (r.prerelease && !draft.includePrereleases) {
          prereleasesSkipped++;
          return false;
        }
        if (draft.filter.prefix && !r.version.startsWith(draft.filter.prefix)) {
          namesSkipped++;
          return false;
        }
        return true;
      });
      return { ...empty, releases: releases.slice(0, 20), withoutVersion: ['README.txt', 'checksums.sha256'], prereleasesSkipped, namesSkipped, tookMs: 380 };
    });
  }

  saveWatch(draft: WatchDraft, secrets: Record<string, string>): Promise<WatchSummary> {
    return this.call('saveWatch', () => {
      const problems: string[] = [];
      if (!draft.name.trim()) problems.push('Give the watch a name.');
      const kind = SOURCE_KINDS.find((k) => k.id === draft.source.kind);
      for (const f of kind?.fields ?? []) {
        if (f.required && f.kind !== 'secret' && !(draft.source.values[f.key] ?? '').trim()) problems.push(`${f.label} is required.`);
      }
      if (this.watches.some((w) => w.spec.name.toLowerCase() === draft.name.trim().toLowerCase() && w.spec.id !== draft.id)) {
        problems.push(`A watch named "${draft.name.trim()}" already exists.`);
      }
      problems.push(...mockScheduleProblems(draft.schedule));
      if (problems.length) throw problem('invalid', 'The watch cannot be saved yet.', problems);
      const clean = (w: MockWatch) => {
        for (const [k, v] of Object.entries(secrets)) {
          w.secrets = w.secrets.filter((s) => s !== k);
          if (v !== '') w.secrets.push(k);
        }
      };
      const spec = { ...structuredClone(draft), name: draft.name.trim() };
      if (draft.id) {
        const w = this.find(draft.id);
        w.spec = spec;
        clean(w);
        this.emit();
        return this.summary(w);
      }
      spec.id = `w-${this.nextId++}`;
      const releases = makeReleases(this.nextId * 31, spec.versions, 9, this.now(), 120, '').reverse();
      const w: MockWatch = { spec, releases, checks: [], ack: null, installed: null, status: { kind: 'pending', message: null }, checking: false, lastCheckMs: null, failures: 0, secrets: [], products: [] };
      clean(w);
      this.watches = [...this.watches, w];
      this.emit();
      return this.summary(w);
    });
  }

  deleteWatch(id: string): Promise<void> {
    return this.call('deleteWatch', () => {
      this.find(id);
      this.watches = this.watches.filter((w) => w.spec.id !== id);
      this.emit();
    });
  }

  setPaused(id: string, paused: boolean): Promise<void> {
    return this.call('setPaused', () => {
      this.find(id).spec.paused = paused;
      this.emit();
    });
  }

  checkNow(id: string | null): Promise<void> {
    return this.call('checkNow', () => {
      const targets = id === null ? this.watches.filter((w) => !w.spec.paused) : [this.find(id)];
      for (const w of targets) {
        w.lastCheckMs = this.now();
        if (w.status.kind === 'pending') w.status = OK;
        w.checks = [...w.checks, { atMs: this.now(), tookMs: 640, error: null, found: w.releases.length }].slice(-30);
      }
      this.emit();
    });
  }

  acknowledge(id: string, version: string | null): Promise<void> {
    return this.call('acknowledge', () => {
      const w = this.find(id);
      w.ack = version ?? this.latest(w)?.version ?? w.ack;
      this.emit();
    });
  }

  resetAcknowledgement(id: string): Promise<void> {
    return this.call('resetAcknowledgement', () => {
      this.find(id).ack = null;
      this.emit();
    });
  }

  setInstalled(id: string, version: string | null): Promise<void> {
    return this.call('setInstalled', () => {
      this.find(id).installed = version;
      this.emit();
    });
  }

  acknowledgeProduct(id: string, product: string): Promise<void> {
    return this.call('acknowledgeProduct', () => {
      const p = this.find(id).products?.find((x) => x.id === product);
      if (!p) throw problem('notFound', 'That product no longer exists.');
      p.unseen = false;
      this.emit();
    });
  }

  openRelease(id: string, version: string, target?: ReleaseTarget): Promise<void> {
    return this.call('openRelease', () => {
      this.release(id, version, target);
    });
  }

  downloadRelease(id: string, version: string, target?: ReleaseTarget): Promise<string> {
    return this.call('downloadRelease', () => {
      const r = this.release(id, version, target);
      const file = target?.artifact ?? r.name;
      return `/home/demo/Downloads/Spotter-demo/${file}`;
    });
  }

  suggestPattern(example: string, selections: PatternSelection[]): Promise<string> {
    return this.call('suggestPattern', () => {
      try {
        return mockSuggest(example, selections);
      } catch (e) {
        throw problem('invalid', (e as Error).message, [(e as Error).message]);
      }
    });
  }

  testPattern(versions: VersionSchemeId, versionPattern: string | null, missingOlder: boolean, text: string): Promise<PatternTest> {
    return this.call('testPattern', () => {
      void missingOlder;
      return mockTest({ versions, versionPattern }, text);
    });
  }

  previewSchedule(schedule: Schedule, sourceKind: string): Promise<SchedulePreview> {
    return this.call('previewSchedule', () => {
      void sourceKind;
      const problems = mockScheduleProblems(schedule);
      if (problems.length) throw problem('invalid', problems.join(' '), problems);
      return mockSchedulePreview(schedule, this.now());
    });
  }

  versionSchemes(): Promise<SchemePreset[]> {
    return this.call('versionSchemes', () => MOCK_SCHEMES);
  }

  dashboard(range: DashboardRange): Promise<Dashboard> {
    return this.call('dashboard', () => {
      const { starts, width } = this.bucketStarts(range);
      const now = this.now();
      const bucketOf = (ms: number) => {
        const i = Math.floor((ms - starts[0]!) / width);
        return i >= 0 && i < starts.length ? i : -1;
      };
      const watches: WatchNumbers[] = this.watches.map((w) => {
        const releases = starts.map(() => 0);
        for (const r of w.releases) {
          const i = bucketOf(r.firstSeenMs);
          if (i >= 0) releases[i]!++;
        }
        const r = rng(Number((w.spec.id ?? 'w-0').slice(2)) + 7);
        const checks = starts.map(() => Math.round((width / DAY) * (w.spec.paused ? 0 : 3)));
        const failedChecks = checks.map((c) => (w.failures > 0 && r() < 0.2 ? Math.min(c, 1 + Math.floor(r() * 2)) : 0));
        const sparkline = Array.from({ length: 12 }, (_, i) => w.releases.filter((x) => now - x.firstSeenMs >= (11 - i) * 7 * DAY && now - x.firstSeenMs < (12 - i) * 7 * DAY).length);
        const gaps = w.releases.slice(0, -1).map((x, i) => x.firstSeenMs - w.releases[i + 1]!.firstSeenMs);
        const newest = w.releases[0];
        return {
          watchId: w.spec.id ?? '',
          averageIntervalMs: gaps.length ? Math.round(gaps.reduce((a, b) => a + b, 0) / gaps.length) : null,
          sinceLastMs: newest ? now - newest.firstSeenMs : null,
          releases,
          failedChecks,
          checks,
          sparkline,
        };
      });
      const dr = rng(99);
      const downloads: DownloadBucket[] = starts.map(() => {
        const count = dr() < 0.15 ? 1 + Math.floor(dr() * 2) : 0;
        return { count, failed: count && dr() < 0.2 ? 1 : 0, bytes: count * Math.round(30_000_000 + dr() * 90_000_000) };
      });
      const whatsNew = this.watches
        .flatMap((w) => {
          const s = this.summary(w);
          return w.releases.slice(0, 2).map((release) => ({
            watchId: s.id,
            watchName: s.name,
            sourceKind: s.sourceKind,
            release,
            releasedAtMs: release.publishedAtMs ?? release.firstSeenMs,
            newerThanInstalled: s.latest?.version === release.version ? s.newerThanInstalled : null,
            unseen: s.latest?.version === release.version && s.unseen,
          }));
        })
        .sort((a, b) => b.releasedAtMs - a.releasedAtMs)
        .slice(0, 8);
      return { range, bucketsMs: starts, whatsNew, watches, downloads, computeMicros: 420 };
    });
  }

  events(limit: number, beforeMs: number | null): Promise<AppEvent[]> {
    return this.call('events', () => {
      const base = { watchId: null, watchName: null, version: null, name: null, message: null, path: null, size: null, verified: null, action: null };
      const list: AppEvent[] = [];
      for (const w of this.watches) {
        for (const r of w.releases.slice(0, 4)) {
          list.push({ ...base, atMs: r.firstSeenMs, kind: 'new-release', watchId: w.spec.id, watchName: w.spec.name, version: r.version, name: r.name });
        }
        if (w.status.kind !== 'ok' && w.status.kind !== 'pending') {
          const kind = w.status.kind === 'needsYou' ? 'needs-you' : 'check-failed';
          list.push({ ...base, atMs: w.lastCheckMs ?? this.now(), kind, watchId: w.spec.id, watchName: w.spec.name, message: w.status.message });
        }
      }
      const first = this.watches[0];
      if (first?.releases[0]) {
        const r = first.releases[0];
        list.push({ ...base, atMs: r.firstSeenMs + 20 * MINUTE, kind: 'download-finished', watchId: first.spec.id, watchName: first.spec.name, version: r.version, name: r.name, path: `/home/demo/Downloads/Spotter-demo/${r.name}`, size: r.size, verified: true });
        list.push({ ...base, atMs: r.firstSeenMs + HOUR, kind: 'recovered', watchId: first.spec.id, watchName: first.spec.name });
      }
      return list
        .filter((e) => beforeMs === null || e.atMs < beforeMs)
        .sort((a, b) => b.atMs - a.atMs)
        .slice(0, limit);
    });
  }

  getSettings(): Promise<Settings> {
    return this.call('getSettings', () => ({ ...this.settings }));
  }

  saveSettings(settings: Settings): Promise<Settings> {
    return this.call('saveSettings', () => {
      if (settings.defaultIntervalSecs < 60) throw problem('invalid', 'The default interval is too short.', ['Use at least one minute.']);
      this.settings = { ...settings };
      return { ...this.settings };
    });
  }

  pauseNotifications(minutes: number | null): Promise<Settings> {
    return this.call('pauseNotifications', () => {
      this.settings = { ...this.settings, notificationsPausedUntilMs: minutes === null ? null : this.now() + minutes * MINUTE };
      return { ...this.settings };
    });
  }

  getAutostart(): Promise<boolean> {
    return this.call('getAutostart', () => this.autostart);
  }

  setAutostart(enabled: boolean): Promise<boolean> {
    return this.call('setAutostart', () => (this.autostart = enabled));
  }

  open(target: OpenTarget): Promise<void> {
    return this.call(`open:${target}`, () => undefined);
  }

  chooseFolder(): Promise<string | null> {
    return this.call('chooseFolder', () => '/home/demo/Downloads/Spotter-demo');
  }

  chooseProgram(): Promise<string | null> {
    return this.call('chooseProgram', () => '/usr/local/bin/installer');
  }

  presets(): Promise<Preset[]> {
    return this.call('presets', () => PRESETS);
  }

  discoverFeeds(url: string): Promise<DiscoveredFeed[]> {
    return this.call('discoverFeeds', () => {
      if (!/^https?:\/\//i.test(url.trim())) throw problem('invalid', 'That is not a web address.', ['Start with http:// or https://']);
      if (/nofeed/i.test(url)) return [];
      const base = url.trim().replace(/\/+$/, '');
      return [
        { url: `${base}/releases.atom`, title: 'Releases' },
        { url: `${base}/blog/feed.xml`, title: 'Blog' },
      ];
    });
  }

  rendererStatus(): Promise<RendererStatus> {
    return this.call('rendererStatus', () => this.renderer);
  }

  installRenderer(): Promise<RendererStatus> {
    return this.call('installRenderer', () => {
      this.renderer = { kind: 'chromium-downloaded', detail: 'Chromium 130.0 (downloaded, checksum verified)', canDownload: false };
      return this.renderer;
    });
  }

  importList(format: ImportFormat, text: string): Promise<ImportResult> {
    return this.call('importList', () => {
      const skipped: string[] = [];
      const drafts: WatchDraft[] = [];
      for (const raw of text.split(/\r?\n/)) {
        const line = raw.trim();
        if (!line || line.startsWith('#')) continue;
        const name = format === 'brewfile' ? /^(?:brew|cask)\s+"([^"]+)"/.exec(line)?.[1] : format === 'opml' ? /text="([^"]+)"/.exec(line)?.[1] : line;
        if (!name) {
          skipped.push(line);
          continue;
        }
        drafts.push(blankDraft({ name, source: { kind: 'github', values: { repo: `${name}/${name}` } } }));
      }
      return { drafts, skipped };
    });
  }

  watchAvatar(id: string): Promise<Avatar | null> {
    return this.call(`watchAvatar:${id}`, () => mockMark(this.find(id).spec.source.kind));
  }

  presetAvatar(id: string): Promise<Avatar | null> {
    return this.call(`presetAvatar:${id}`, () => {
      const preset = PRESETS.find((p) => p.id === id);
      if (!preset) throw problem('notFound', `There is no preset "${id}".`);
      return mockMark(preset.spec.source.kind);
    });
  }

  kindAvatar(kind: string): Promise<Avatar | null> {
    return this.call(`kindAvatar:${kind}`, () => mockMark(kind));
  }

  mailStatus(): Promise<MailStatus> {
    return this.call('mailStatus', () => this.mail());
  }

  setMailPassword(password: string | null): Promise<MailStatus> {
    return this.call('setMailPassword', () => {
      this.mailPassword = !!password;
      return this.mail();
    });
  }

  sendTestEmail(to: string): Promise<void> {
    return this.call('sendTestEmail', () => {
      if (!/^[^@\s]+@[^@\s]+\.[^@\s]+$/.test(to.trim())) throw problem('badRequest', 'Enter an e-mail address like name@example.org.');
      if (!this.settings.smtp) throw problem('badRequest', 'Set up the mail server first (server, port and sender), then send the test.');
    });
  }

  private mail(): MailStatus {
    return { configured: !!this.settings.smtp, passwordSet: this.mailPassword };
  }

  appUpdateStatus(): Promise<AppUpdateStatus> {
    return this.call('appUpdateStatus', () => ({ available: true, reason: null, currentVersion: '0.1.0' }));
  }

  checkAppUpdate(): Promise<AppUpdateCheck> {
    return this.call('checkAppUpdate', () => {
      this.updateCount++;
      return this.updateCount % 2 === 1
        ? { updateAvailable: true, version: '0.2.0', notes: 'Faster checks and a calmer dashboard.' }
        : { updateAvailable: false, version: null, notes: null };
    });
  }

  installAppUpdate(): Promise<void> {
    return this.call('installAppUpdate', () => undefined);
  }

  showMainWindow(route: string | null): Promise<void> {
    return this.call('showMainWindow', () => {
      if (route) this.navigate(route);
    });
  }

  hideTray(): Promise<void> {
    return this.call('hideTray', () => undefined);
  }

  async onState(handler: (push: StatePush) => void): Promise<() => void> {
    this.stateHandlers.add(handler);
    return () => this.stateHandlers.delete(handler);
  }

  async onNavigate(handler: (route: string) => void): Promise<() => void> {
    this.navigateHandlers.add(handler);
    return () => this.navigateHandlers.delete(handler);
  }
}
