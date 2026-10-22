import type {
  ActivityReply,
  AppUpdateInfo,
  AppUpdateStatus,
  BackendEvent,
  CleanupReport,
  CommandError,
  Config,
  ConnectionState,
  DaySummary,
  DiskReply,
  JobDay,
  LogLine,
  MetricsReply,
  PitcrewNotification,
  RegisterRequest,
  RunDetails,
  RunnerConfig,
  RunnerSnapshot,
  ServiceInstall,
  SettingsReply,
  StatusReply,
  UpdatePlan,
  Version,
} from './api.types';
import type { Platform, PitcrewBackend } from './backend';
import {
  DAY,
  FORGEJO_SERVER,
  GB,
  HOUR,
  MINUTE,
  PROFILES,
  RawHistory,
  SERVER,
  cleanupPreview,
  clipSpans,
  defaultConfig,
  demoRunnerConfigs,
  demoSnapshots,
  diskScan,
  folderSeries,
  generateHistory,
  isoLocal,
  lastCleanup,
  logLines,
  machineSamples,
  startOfLocalDay,
} from './mock/mock-data';

/** Which world the mock shows. */
export type MockScenario = 'demo' | 'empty' | 'unavailable' | 'notInstalled' | 'mismatch';

export interface MockOptions {
  scenario?: MockScenario;
  /** Simulated latency of every call (0 in tests). */
  latencyMs?: number;
  /** Whether `watch()` starts the simulated push stream (status ticks, notifications, update progress). */
  simulate?: boolean;
  platform?: Platform;
  now?: () => number;
}

const FLOW: Exclude<RunnerSnapshot['updatePhase']['phase'], 'idle' | 'rollingBack'>[] = [
  'downloading',
  'verifying',
  'draining',
  'stopping',
  'swapping',
  'confirming',
];

const LOG_CAP = 5_000;

function fail(code: CommandError['code'], message: string): CommandError {
  return { code, message };
}

/**
 * The browser backend: invented, plausible data for development, UI tests and screenshots. Actions change the demo
 * state roughly as the service would, so the UI can be exercised, but this is never the reference for behaviour.
 */
export class MockBackend implements PitcrewBackend {
  private readonly scenario: MockScenario;
  private readonly latency: number;
  private readonly simulate: boolean;
  private readonly platformName: Platform;
  private readonly now: () => number;
  private config: Config;
  private runners: RunnerSnapshot[];
  private readonly history = new Map<string, RawHistory>();
  private readonly logStore = new Map<string, LogLine[]>();
  private readonly tokens = new Set<string>([SERVER]);
  private readonly listeners = new Set<(event: BackendEvent) => void>();
  private readonly timers = new Set<ReturnType<typeof setTimeout>>();
  private ticker: ReturnType<typeof setInterval> | null = null;
  private ticks = 0;
  private service: ServiceInstall;
  private autostartOn = true;
  private readonly startedAt: number;

  constructor(options: MockOptions = {}) {
    this.scenario = options.scenario ?? 'demo';
    this.latency = options.latencyMs ?? 0;
    this.simulate = options.simulate ?? false;
    this.platformName = options.platform ?? 'browser';
    this.now = options.now ?? Date.now;
    const now = this.now();
    this.startedAt = now - 3 * DAY - 4 * HOUR - 2 * MINUTE;
    const configs = this.scenario === 'empty' ? [] : demoRunnerConfigs();
    this.config = defaultConfig(configs);
    this.runners = demoSnapshots(now, configs, this.config.defaults);
    this.service = {
      platform: 'windows',
      installed: this.scenario !== 'notInstalled',
      running: this.scenario !== 'notInstalled' && this.scenario !== 'unavailable',
      canInstall: true,
      message: null,
    };
  }

  platform(): Platform {
    return this.platformName;
  }

  // ---- status and events -------------------------------------------------------------------------------------------

  async status(): Promise<StatusReply> {
    await this.delay();
    this.ensureService();
    return structuredClone({ runners: this.runners, service: this.serviceInfo() });
  }

  watch(listener: (event: BackendEvent) => void): () => void {
    this.listeners.add(listener);
    queueMicrotask(() => listener({ kind: 'connection', connection: this.connection() }));
    if (this.simulate && !this.ticker && this.scenario === 'demo') {
      this.ticker = setInterval(() => this.tick(), 5_000);
    }
    return () => {
      this.listeners.delete(listener);
      if (this.listeners.size === 0) this.stopSimulation();
    };
  }

  /** Stops every timer (tests, hot reload). */
  dispose(): void {
    this.stopSimulation();
    for (const t of this.timers) clearTimeout(t);
    this.timers.clear();
  }

  // ---- runner actions ----------------------------------------------------------------------------------------------

  async start(runner: string): Promise<void> {
    await this.delay();
    const r = this.find(runner);
    if (r.status !== 'stopped' && r.status !== 'crashLoop' && r.status !== 'notRegistered') throw fail('busy', `${runner} is already running.`);
    this.boot(r);
  }

  async stop(runner: string): Promise<void> {
    await this.delay();
    const r = this.find(runner);
    if (r.currentJob) {
      r.pending = 'stop';
    } else {
      Object.assign(r, { status: 'stopped', session: { kind: 'notRunning' }, runningSince: null, nextStartAt: null, problem: null });
    }
    this.pushStatus();
  }

  async restart(runner: string): Promise<void> {
    await this.delay();
    const r = this.find(runner);
    if (r.currentJob) {
      r.pending = 'restart';
      this.pushStatus();
      return;
    }
    r.restarts24h += 1;
    this.boot(r);
  }

  async drain(runner: string): Promise<void> {
    await this.delay();
    const r = this.find(runner);
    if (r.currentJob) Object.assign(r, { status: 'draining', pending: 'pause' });
    else Object.assign(r, { status: 'paused', pending: null });
    this.pushStatus();
  }

  async resume(runner: string): Promise<void> {
    await this.delay();
    const r = this.find(runner);
    if (r.status !== 'paused' && r.status !== 'draining') throw fail('badRequest', `${runner} is not paused.`);
    Object.assign(r, { status: r.currentJob ? 'busy' : 'idle', pending: null, session: { kind: 'userSession' }, runningSince: r.runningSince ?? this.now() });
    this.pushStatus();
  }

  async checkUpdate(runner: string): Promise<void> {
    await this.delay();
    const r = this.find(runner);
    if (!r.managedBinary) throw fail('badRequest', `${runner} uses its own binary (${r.kind}); Pitcrew does not update it.`);
    r.checkedAt = this.now();
    r.latestVersion = '5.3.0';
    r.updateTarget = r.currentVersion === '5.3.0' || r.updatePolicy.skipped.includes('5.3.0') ? null : (r.updatePolicy.pinned ?? '5.3.0');
    this.pushStatus();
  }

  async applyUpdate(runner: string): Promise<void> {
    await this.delay();
    const r = this.find(runner);
    if (!r.managedBinary) throw fail('badRequest', `${runner} uses its own binary; Pitcrew does not update it.`);
    if (r.updatePhase.phase !== 'idle') throw fail('busy', `An update of ${runner} is already in progress.`);
    const to = r.updateTarget ?? r.latestVersion;
    if (!to || to === r.currentVersion) throw fail('badRequest', `${runner} is up to date.`);
    this.runUpdate(r, { from: r.currentVersion, to });
  }

  async cancelUpdate(runner: string): Promise<void> {
    await this.delay();
    const r = this.find(runner);
    const p = r.updatePhase;
    if (p.phase === 'idle') throw fail('badRequest', `No update of ${runner} is in progress.`);
    if (p.phase === 'swapping' || p.phase === 'confirming' || p.phase === 'rollingBack') throw fail('busy', 'The binary is being swapped; the update can no longer be cancelled.');
    this.clearTimers();
    r.lastUpdate = { outcome: { result: 'cancelled', plan: p.plan }, finishedAt: this.now() };
    r.updatePhase = { phase: 'idle' };
    if (r.status === 'updating' || r.status === 'draining') r.status = r.currentJob ? 'busy' : 'idle';
    this.pushStatus();
  }

  async skipVersion(runner: string, version: Version): Promise<void> {
    await this.delay();
    const r = this.find(runner);
    if (!r.updatePolicy.skipped.includes(version)) r.updatePolicy.skipped.push(version);
    if (r.updateTarget === version) r.updateTarget = null;
    this.syncConfig(r);
    this.pushStatus();
  }

  async pinVersion(runner: string, version: Version | null): Promise<void> {
    await this.delay();
    const r = this.find(runner);
    if (version !== null && !/^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$/.test(version)) throw fail('badRequest', `"${version}" is not a version like 5.2.0.`);
    r.updatePolicy.pinned = version;
    r.updateTarget = version !== null ? (version === r.currentVersion ? null : version) : r.latestVersion !== r.currentVersion ? r.latestVersion : null;
    this.syncConfig(r);
    this.pushStatus();
  }

  // ---- logs --------------------------------------------------------------------------------------------------------

  async logs(runner: string, after: number | null, limit: number): Promise<LogLine[]> {
    await this.delay();
    const lines = this.logsOf(runner);
    const newer = after === null ? lines : lines.filter((l) => l.seq > after);
    return structuredClone(after === null ? newer.slice(-limit) : newer.slice(0, limit));
  }

  async searchLogs(runner: string, query: string, limit: number): Promise<LogLine[]> {
    await this.delay();
    const q = query.trim().toLowerCase();
    if (!q) return [];
    return structuredClone(this.logsOf(runner).filter((l) => l.text.toLowerCase().includes(q)).slice(-limit));
  }

  // ---- activity, metrics and disk ----------------------------------------------------------------------------------

  async activity(runner: string, days: number): Promise<ActivityReply> {
    await this.delay();
    const r = this.find(runner);
    const now = this.now();
    const h = this.historyOf(r.name);
    const from = startOfLocalDay(now) - (Math.max(1, days) - 1) * DAY;
    const jobs = h.jobs.filter((j) => j.finishedAt >= from);
    const up = clipSpans(h.up, from, now);
    const busy = clipSpans(
      jobs.map((j) => ({ from: j.startedAt, to: j.finishedAt })),
      from,
      now,
    );
    if (r.currentJob) busy.push({ from: r.currentJob.startedAt, to: now });
    const hasApiToken = r.serverUrl !== null && this.tokens.has(r.serverUrl);
    const dayList: DaySummary[] = [];
    const jobDays: JobDay[] = [];
    for (let start = from; start <= now; start += DAY) {
      const end = Math.min(start + DAY, now);
      const dj = jobs.filter((j) => j.finishedAt >= start && j.finishedAt < start + DAY);
      const upMs = clipSpans(up, start, end).reduce((a, s) => a + s.to - s.from, 0);
      const busyMs = dj.reduce((a, j) => a + j.finishedAt - j.startedAt, 0);
      const restarts = h.marks.filter((m) => m.at >= start && m.at < start + DAY && (m.kind === 'crash' || m.kind === 'stall' || m.kind === 'crashLoop')).length;
      dayList.push({ start, jobs: dj.length, restarts, uptimePermille: Math.round((upMs / Math.max(1, end - start)) * 1000), busyMs, avgJobMs: dj.length ? Math.round(busyMs / dj.length) : 0 });
      const count = (o: string) => dj.filter((j) => j.outcome === o).length;
      jobDays.push(
        hasApiToken
          ? { start, success: count('success'), failure: count('failure'), cancelled: count('cancelled'), unknown: 0, avgQueuedMs: dj.length ? Math.round(dj.reduce((a, j) => a + j.queuedMs, 0) / dj.length) : null }
          : { start, success: 0, failure: 0, cancelled: 0, unknown: dj.length, avgQueuedMs: null },
      );
    }
    const upMs = up.reduce((a, s) => a + s.to - s.from, 0);
    return structuredClone({
      summary: {
        from,
        to: now,
        days: dayList,
        uptimePermille: Math.round((upMs / Math.max(1, now - from)) * 1000),
        restarts: dayList.reduce((a, d) => a + d.restarts, 0),
        jobs: jobs.length,
        up,
        busy,
        marks: h.marks.filter((m) => m.at >= from),
        updates: h.updates.filter((u) => u.at >= from),
        recentJobs: jobs.slice(-40).map(({ info, startedAt, finishedAt }) => ({ info, startedAt, finishedAt })).reverse(),
      },
      jobDays,
      events: h.events.filter((e) => e.at >= from).slice(0, 200),
      hasApiToken,
    });
  }

  async metrics(runner: string | null, rangeHours: number): Promise<MetricsReply> {
    await this.delay();
    this.ensureService();
    const now = this.now();
    const hours = Math.min(Math.max(1, rangeHours), 24 * 30);
    const r = runner === null ? null : this.find(runner);
    const base = r?.disk ? (r.disk.folderBytes / GB) * 0.75 : 0;
    return { machine: machineSamples(now, hours), folder: r ? folderSeries(r.name, now, hours, base) : [] };
  }

  async disk(runner: string): Promise<DiskReply> {
    await this.delay();
    const r = this.find(runner);
    const now = this.now();
    const scan = diskScan(r, now);
    const conf = this.config.runners.find((c) => c.name === runner);
    return structuredClone({ scan, preview: cleanupPreview(scan), lastCleanup: lastCleanup(now), policy: conf?.cleanup ?? this.config.defaults.cleanup });
  }

  async cleanDisk(runner: string): Promise<CleanupReport> {
    await this.delay(this.latency * 6);
    const r = this.find(runner);
    const preview = cleanupPreview(diskScan(r, this.now()));
    const freed = preview.freedBytes;
    if (r.disk) {
      r.disk.folderBytes = Math.max(GB, r.disk.folderBytes - freed);
      if (r.disk.freeBytes !== null) r.disk.freeBytes += freed;
      r.disk.low = false;
    }
    const report: CleanupReport = {
      trigger: 'manual',
      at: this.now(),
      freedBytes: freed,
      deleted: 412,
      failures: [{ path: 'work\\_tool\\tool_cache\\node\\22.11.0\\x64\\node.exe', reason: 'access is denied (a file is read-only, in use, or owned by another account)' }],
    };
    this.historyOf(runner).events.unshift({ at: report.at, kind: 'cleanup', trigger: 'manual', freedBytes: freed, deleted: report.deleted, failed: 1 });
    this.pushStatus();
    return report;
  }

  async runDetails(runner: string): Promise<RunDetails | null> {
    await this.delay();
    const r = this.find(runner);
    if (!r.currentJob || r.serverUrl === null || !this.tokens.has(r.serverUrl)) return null;
    const run = r.currentJob.info.runNumber ?? 1;
    return {
      title: 'Retry declined card payments at checkout',
      workflow: 'ci.yml',
      branch: 'feature/checkout-retry',
      event: 'pull_request',
      url: `${r.serverUrl}/${r.currentJob.info.repository ?? 'acme/storefront'}/actions/runs/${run}`,
    };
  }

  // ---- settings and runners ----------------------------------------------------------------------------------------

  async settings(): Promise<SettingsReply> {
    await this.delay();
    this.ensureService();
    return structuredClone({ config: this.config, problems: this.problems(), readOnly: false });
  }

  async saveSettings(config: Config): Promise<void> {
    await this.delay();
    if (config.defaults.watchdog.threshold < 1) throw fail('badRequest', 'The stall threshold must be at least 1 timed-out poll.');
    this.config = structuredClone(config);
    for (const r of this.runners) {
      const c = this.config.runners.find((x) => x.name === r.name);
      if (!c) continue;
      r.updatePolicy = structuredClone(c.update ?? this.config.defaults.update);
      r.labels = [...c.labels];
      r.sessionMode = c.session;
      r.enabled = c.enabled;
    }
    this.pushStatus();
  }

  async addRunner(runner: RunnerConfig): Promise<void> {
    await this.delay();
    if (this.config.runners.some((r) => r.name === runner.name)) throw fail('badRequest', `A runner named ${runner.name} exists already.`);
    this.config.runners.push(structuredClone(runner));
    const snap = demoSnapshots(this.now(), [{ ...runner, name: 'build-win' }], this.config.defaults)[0];
    if (snap) this.runners.push({ ...snap, name: runner.name, status: 'notRegistered', currentJob: null, problem: 'The runner is not registered with a server yet.' });
    this.pushStatus();
  }

  async removeRunner(runner: string): Promise<void> {
    await this.delay();
    this.find(runner);
    this.runners = this.runners.filter((r) => r.name !== runner);
    this.config.runners = this.config.runners.filter((r) => r.name !== runner);
    this.pushStatus();
  }

  async register(request: RegisterRequest): Promise<void> {
    await this.delay(this.latency * 8);
    if (!/^[a-z0-9][a-z0-9-]{0,31}$/.test(request.runner)) throw fail('badRequest', 'The name may only use a–z, 0–9 and "-" (at most 32 characters).');
    if (!/^https?:\/\//.test(request.serverUrl)) throw fail('badRequest', 'The server URL must start with https://.');
    if (request.token.trim().length < 8) throw fail('badRequest', 'The server refused the registration token. Copy a fresh one from the server’s runner settings.');
    if (this.runners.some((r) => r.name === request.runner)) throw fail('badRequest', `A runner named ${request.runner} exists already.`);
    const dir = request.dir ?? `C:\\ProgramData\\Pitcrew\\runners\\${request.runner}`;
    const conf: RunnerConfig = {
      name: request.runner,
      kind: 'gitea-runner',
      dir,
      labels: request.labels,
      session: request.session,
      logged_off: request.loggedOff,
      server_url: request.serverUrl,
      enabled: true,
    };
    this.config.runners.push(conf);
    const snap = demoSnapshots(this.now(), [{ ...conf, name: 'ui-tests' }], this.config.defaults)[0];
    if (!snap) return;
    const r: RunnerSnapshot = { ...snap, name: request.runner, dir, labels: [...request.labels], latestVersion: '5.3.0', updateTarget: null, currentVersion: '5.3.0', lastUpdate: null, restarts24h: 0, disk: { folderBytes: 0.2 * GB, freeBytes: 196 * GB, low: false } };
    this.runners.push(r);
    this.boot(r);
  }

  async hasApiToken(serverUrl: string): Promise<boolean> {
    await this.delay();
    return this.tokens.has(serverUrl);
  }

  async setApiToken(serverUrl: string, token: string): Promise<void> {
    await this.delay();
    if (token.trim().length < 8) throw fail('badRequest', 'That does not look like a Gitea access token.');
    this.tokens.add(serverUrl);
  }

  async clearApiToken(serverUrl: string): Promise<void> {
    await this.delay();
    this.tokens.delete(serverUrl);
  }

  // ---- the service's registration ----------------------------------------------------------------------------------

  async serviceInstall(): Promise<ServiceInstall> {
    await this.delay();
    return structuredClone(this.service);
  }

  async installService(): Promise<void> {
    await this.delay(this.latency * 6);
    this.service = { ...this.service, installed: true, running: true, message: null };
    this.emit({ kind: 'connection', connection: this.connection() });
    this.pushStatus();
  }

  async uninstallService(): Promise<void> {
    await this.delay(this.latency * 4);
    this.service = { ...this.service, installed: false, running: false };
    this.emit({ kind: 'connection', connection: this.connection() });
  }

  // ---- the app -----------------------------------------------------------------------------------------------------

  async appUpdateStatus(): Promise<AppUpdateStatus> {
    await this.delay();
    return { available: false, reason: 'Updates are not available in this build (it has no update key).', currentVersion: '0.1.0' };
  }

  async checkAppUpdate(): Promise<AppUpdateInfo | null> {
    await this.delay();
    throw fail('notImplemented', 'Updates are not available in this build (it has no update key).');
  }

  async installAppUpdate(): Promise<void> {
    await this.delay();
    throw fail('notImplemented', 'Updates are not available in this build (it has no update key).');
  }

  async autostart(): Promise<boolean> {
    await this.delay();
    return this.autostartOn;
  }

  async setAutostart(enabled: boolean): Promise<void> {
    await this.delay();
    this.autostartOn = enabled;
  }

  async exportDiagnostics(): Promise<string> {
    await this.delay(this.latency * 4);
    const stamp = isoLocal(this.now()).slice(0, 19).replace(/[:T]/g, '-');
    return `C:\\Users\\demo\\Downloads\\pitcrew-diagnostics-${stamp}.zip`;
  }

  async openUrl(url: string): Promise<void> {
    await this.delay();
    if (!/^https?:\/\//.test(url)) throw fail('badRequest', 'Only http(s) links can be opened.');
    if (typeof window !== 'undefined' && typeof window.open === 'function') window.open(url, '_blank', 'noopener');
  }

  async revealLogs(runner: string): Promise<void> {
    await this.delay();
    this.find(runner);
  }

  // ---- internals ---------------------------------------------------------------------------------------------------

  private connection(): ConnectionState {
    if (this.scenario === 'mismatch') return { state: 'mismatch', message: 'The Pitcrew service is version 0.2.0 and speaks protocol 3; this app speaks protocol 2. Install the same Pitcrew version for both.' };
    if (!this.service.installed) return { state: 'unavailable', message: 'The Pitcrew service is not installed on this computer.' };
    if (!this.service.running) return { state: 'unavailable', message: 'The Pitcrew service is installed but not running.' };
    return { state: 'connected', serviceVersion: '0.1.0' };
  }

  private ensureService(): void {
    const c = this.connection();
    if (c.state === 'mismatch') throw fail('protocolMismatch', c.message);
    if (c.state === 'unavailable') throw fail('serviceUnavailable', c.message);
  }

  private serviceInfo(): StatusReply['service'] {
    const now = this.now();
    const machine = machineSamples(now, 1).at(-1) ?? null;
    return {
      version: '0.1.0',
      startedAt: this.startedAt,
      dataDir: 'C:\\ProgramData\\Pitcrew',
      configProblems: this.problems(),
      readOnly: false,
      machine,
      ownCpuPermille: 3 + (this.ticks % 3),
      ownMemBytes: Math.round(23.4 * 1024 * 1024),
    };
  }

  private problems(): string[] {
    return this.scenario === 'demo' ? ['runners[4] ("old-box"): unknown key "binary" — did you mean "binary_path"? The entry was skipped.'] : [];
  }

  private find(name: string): RunnerSnapshot {
    this.ensureService();
    const r = this.runners.find((x) => x.name === name);
    if (!r) throw fail('unknownRunner', `There is no runner named ${name}.`);
    return r;
  }

  private historyOf(name: string): RawHistory {
    let h = this.history.get(name);
    if (!h) {
      const profile = PROFILES.find((p) => p.name === name) ?? { ...PROFILES[1]!, name, seed: name.length * 13, updates: [], outages: [] };
      h = generateHistory(profile, this.now());
      this.history.set(name, h);
    }
    return h;
  }

  private logsOf(name: string): LogLine[] {
    const r = this.find(name);
    let lines = this.logStore.get(name);
    if (!lines) {
      lines = logLines(r, this.now(), 1_200).map((text, i) => ({ seq: i + 1, text }));
      this.logStore.set(name, lines);
    }
    return lines;
  }

  private appendLog(name: string, level: string, msg: string): void {
    const lines = this.logsOf(name);
    const seq = (lines.at(-1)?.seq ?? 0) + 1;
    lines.push({ seq, text: `time="${isoLocal(this.now())}" level=${level} msg="${msg}"` });
    if (lines.length > LOG_CAP) lines.splice(0, lines.length - LOG_CAP);
  }

  private boot(r: RunnerSnapshot): void {
    Object.assign(r, { status: 'starting', session: { kind: 'userSession' }, runningSince: this.now(), nextStartAt: null, problem: null, pending: null });
    this.pushStatus();
    this.later(1_500, () => {
      if (r.status !== 'starting') return;
      r.status = 'idle';
      this.appendLog(r.name, 'info', `runner: ${r.name}, with version: v${r.currentVersion ?? '5.2.0'}, with labels: [${r.labels.join(' ')}], declare successfully`);
      this.pushStatus();
    });
  }

  private runUpdate(r: RunnerSnapshot, plan: UpdatePlan): void {
    const step = (i: number) => {
      const phase = FLOW[i];
      if (!phase) {
        r.updatePhase = { phase: 'idle' };
        r.currentVersion = plan.to;
        r.updateTarget = null;
        r.status = 'idle';
        r.runningSince = this.now();
        r.lastUpdate = { outcome: { result: 'updated', plan }, finishedAt: this.now() };
        this.notify({ kind: 'updated', severity: 'info', runner: r.name, title: `${r.name} updated to ${plan.to}`, body: `The server accepted the new runner. The previous binary (${plan.from ?? 'none'}) is kept for roll-back.`, at: this.now() });
        this.pushStatus();
        return;
      }
      const since = this.now();
      r.updatePhase =
        phase === 'draining' || phase === 'stopping' || phase === 'confirming' ? { phase, plan, since } : { phase, plan };
      if (phase === 'draining' && r.currentJob) r.status = 'draining';
      if (phase === 'stopping' || phase === 'swapping' || phase === 'confirming') r.status = 'updating';
      this.pushStatus();
      this.later(phase === 'draining' && r.currentJob ? 30_000 : 1_800, () => step(i + 1));
    };
    step(0);
  }

  private syncConfig(r: RunnerSnapshot): void {
    const c = this.config.runners.find((x) => x.name === r.name);
    if (c) c.update = structuredClone(r.updatePolicy);
  }

  private tick(): void {
    this.ticks += 1;
    const busy = this.runners.find((r) => r.currentJob);
    if (busy) this.appendLog(busy.name, 'info', `[ci/build] ${['Compiling storefront (Release|x64)', 'Linking storefront.exe', 'Running 1,284 tests', 'Packing artifacts'][this.ticks % 4]}`);
    if (this.ticks === 5) {
      this.notify({ kind: 'cleanedUp', severity: 'info', runner: 'docker-lab', title: 'Freed 21.4 GB on docker-lab', body: 'The disk was low (18.6 GB free). Pitcrew deleted work folders older than 3 days and the old tool cache.', at: this.now() });
    }
    if (this.ticks % 36 === 0) {
      this.notify({ kind: 'serverUnreachable', severity: 'warning', runner: 'build-win', title: 'git.example.com is not answering', body: 'build-win could not reach the server for 2 minutes. It keeps retrying; running jobs continue.', at: this.now() });
    }
    this.pushStatus();
  }

  private notify(n: PitcrewNotification): void {
    this.emit({ kind: 'notification', notification: n });
  }

  private pushStatus(): void {
    if (this.listeners.size === 0) return;
    try {
      this.emit({ kind: 'status', status: structuredClone({ runners: this.runners, service: this.serviceInfo() }) });
    } catch {
      // Not connected: nothing to push.
    }
  }

  private emit(event: BackendEvent): void {
    for (const l of this.listeners) l(event);
  }

  private later(ms: number, fn: () => void): void {
    const t = setTimeout(() => {
      this.timers.delete(t);
      fn();
    }, ms);
    this.timers.add(t);
  }

  private clearTimers(): void {
    for (const t of this.timers) clearTimeout(t);
    this.timers.clear();
  }

  private stopSimulation(): void {
    if (this.ticker) clearInterval(this.ticker);
    this.ticker = null;
  }

  private delay(ms = this.latency): Promise<void> {
    return ms > 0 ? new Promise((resolve) => setTimeout(resolve, ms)) : Promise.resolve();
  }
}

/** The demo's Forgejo server, which has no read token in the demo (outcomes show as unknown). */
export const DEMO_SERVERS = [SERVER, FORGEJO_SERVER];
