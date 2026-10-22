import type {
  AppInfo,
  AppPermissions,
  AppUpdateStatus,
  BenchmarkResult,
  CommandError,
  CreatedKey,
  DiskUsage,
  Download,
  Engine,
  EngineIssue,
  History,
  HistoryRange,
  HfModel,
  HfRepo,
  Job,
  LiveUpdate,
  LoadedModel,
  LocalModel,
  MemorySample,
  Overview,
  PacksInfo,
  PairingRequest,
  PitwallEvent,
  PlaygroundEvent,
  PlaygroundRequest,
  Preset,
  QueueSnapshot,
  RequestContent,
  RequestRecord,
  ServiceInstall,
  UpdateInfo,
  ServerInfo,
  Settings,
  Throughput,
} from './api.types';
import type { PitwallBackend, Unsubscribe } from './backend';
import {
  DEMO_REPLY,
  GPU_NAME,
  GiB,
  MiB,
  RAM_TOTAL,
  VRAM_TOTAL,
  demoApps,
  demoBenchmarks,
  demoEngines,
  demoEvents,
  demoFit,
  demoHistory,
  demoLoaded,
  demoLocalModels,
  demoPacks,
  demoPairings,
  demoRepo,
  demoRequests,
  demoSearch,
  demoSettings,
  presetMemory,
  prng,
} from './mock-data';

export interface MockOptions {
  /** Reference time for all demo data (default: now). */
  now?: () => number;
  /** Simulated latency of every call (ms). */
  latencyMs?: number;
  /** Push live updates every second while someone is subscribed (off in unit tests, which call `tick()`). */
  autoTick?: boolean;
  /** Methods that reject with `serverUnavailable` (error-state tests and screenshots). */
  failing?: readonly (keyof PitwallBackend)[];
  /** Start with no models, apps or history (empty-state tests). */
  empty?: boolean;
  seed?: number;
  /** The server is not registered with the OS yet and the user has not been asked (first-run question). */
  askConsent?: boolean;
  /** A newer release is available and the build can update itself. */
  updateAvailable?: boolean;
  /** A model's engine crashed repeatedly and was given up on (dashboard issue with Retry). */
  crashed?: boolean;
  /** Delay between streamed playground words (ms). */
  streamDelayMs?: number;
}

const clone = <T>(v: T): T => structuredClone(v);

/**
 * The browser-build backend: realistic, deterministic demo data and simulated live updates. Browser development and
 * UI tests only; it mimics the contract and is never the reference for behaviour.
 */
export class MockBackend implements PitwallBackend {
  private readonly now: () => number;
  private readonly latency: number;
  private readonly autoTick: boolean;
  private readonly failing: ReadonlySet<string>;
  private readonly rnd: () => number;
  private readonly streamDelay: number;

  private server: ServerInfo;
  private memory: MemorySample;
  private loaded: LoadedModel[];
  private library: LocalModel[];
  private apps_: AppInfo[];
  private pairings_: PairingRequest[];
  private queue: QueueSnapshot;
  private throughput: Throughput;
  private events_: PitwallEvent[];
  private settings_: Settings;
  private engines_: Engine[];
  private downloads_: Download[] = [];
  private readonly requests_: RequestRecord[];
  private readonly benchmarks_: BenchmarkResult[];
  private readonly histories = new Map<HistoryRange, History>();
  private readonly listeners = new Set<(u: LiveUpdate) => void>();
  private timer: ReturnType<typeof setInterval> | null = null;
  private ticks = 0;
  private nextId = 1;
  private service: ServiceInstall;
  private autostart_ = false;
  private issues_: EngineIssue[] = [];
  private readonly updateAvailable: boolean;

  constructor(options: MockOptions = {}) {
    this.now = options.now ?? (() => Date.now());
    this.latency = options.latencyMs ?? 0;
    this.autoTick = options.autoTick ?? false;
    this.failing = new Set(options.failing ?? []);
    this.rnd = prng(options.seed ?? 42);
    this.streamDelay = options.streamDelayMs ?? 0;
    this.updateAvailable = options.updateAvailable ?? false;
    if (options.crashed) {
      this.issues_ = [
        { modelId: 'gemma-3-12b-it-q4_k_m', state: 'failed', crashes: 6, restartAt: null, message: 'The engine for gemma-3-12b-it-q4_k_m crashed repeatedly and Pitwall stopped restarting it. Check that the model file is complete, then press Retry.' },
      ];
    }
    this.service = {
      platform: 'macos',
      installed: !options.askConsent,
      running: true,
      canInstall: true,
      askConsent: options.askConsent ?? false,
      message: null,
    };
    const now = this.now();
    const empty = options.empty ?? false;
    this.settings_ = demoSettings();
    this.server = {
      version: '0.1.0',
      apiVersion: 1,
      pid: 7412,
      port: this.settings_.server.port,
      startedAt: now - 5 * 3_600_000 - 12 * 60_000,
      serving: 'serving',
      offline: false,
      lan: { enabled: false, url: null, certFingerprint: null },
    };
    this.library = empty ? [] : demoLocalModels(now);
    this.loaded = empty ? [] : demoLoaded(now, this.settings_.memory.idleUnloadSecs);
    this.apps_ = empty ? [] : demoApps(now);
    this.pairings_ = empty ? [] : demoPairings(now);
    this.events_ = empty ? [] : demoEvents(now);
    this.engines_ = demoEngines(now);
    this.requests_ = empty ? [] : demoRequests(now);
    this.benchmarks_ = empty ? [] : demoBenchmarks(now);
    this.memory = this.sampleMemory(now);
    this.queue = empty ? { running: [], waiting: [], depth: { interactive: 0, background: 0, batch: 0 } } : this.demoQueue(now);
    this.throughput = empty ? { promptTps: 0, genTps: 0, ttftP50Ms: 0, requestsPerMin: 0, activeRequests: 0 } : { promptTps: 1184, genTps: 58.4, ttftP50Ms: 214, requestsPerMin: 37, activeRequests: 3 };
    if (empty) for (const r of ['1h', '24h', '7d', '30d'] as const) this.histories.set(r, { range: r, stepMs: 60_000, points: [], requestsByApp: [], requestsByModel: [], events: [] });
    if (!empty) {
      this.downloads_ = [
        { id: 'dl-1', kind: 'model', name: 'Qwen3-14B-Instruct-Q4_K_M.gguf', host: 'huggingface.co', bytesDone: 3.1 * GiB, bytesTotal: 8.38 * GiB, speedBps: 38 * MiB, state: 'downloading', error: null, startedAt: now - 90_000 },
        { id: 'dl-0', kind: 'model', name: 'Phi-4-mini-instruct-Q4_K_M.gguf', host: 'huggingface.co', bytesDone: 1.02 * GiB, bytesTotal: 2.49 * GiB, speedBps: 0, state: 'paused', error: 'Connection reset; resumes from 1.0 GB', startedAt: now - 3_600_000 },
      ];
    }
  }

  // ---- Simulation --------------------------------------------------------------------------------------------

  private sampleMemory(now: number): MemorySample {
    const modelRam = this.loaded.reduce((s, m) => s + m.ramBytes, 0);
    const modelVram = this.loaded.reduce((s, m) => s + m.vramBytes, 0);
    const wobble = Math.sin(now / 9_000) * 0.25 * GiB + (this.rnd() - 0.5) * 0.15 * GiB;
    const used = 15.9 * GiB + modelRam + wobble;
    const available = RAM_TOTAL - used;
    const pct = available / RAM_TOTAL;
    return {
      at: now,
      ramTotal: RAM_TOTAL,
      ramAvailable: available,
      commitUsed: used + 3.2 * GiB,
      commitLimit: RAM_TOTAL + 8 * GiB,
      swapUsed: 0.6 * GiB,
      pressure: pct < 0.1 ? 'critical' : pct < 0.2 ? 'elevated' : 'normal',
      cpuPercent: Math.round((14 + this.rnd() * 12 + (this.throughput?.activeRequests ?? 0) * 3) * 10) / 10,
      gpus: [{ name: GPU_NAME, vramTotal: VRAM_TOTAL, vramUsed: 0.86 * GiB + modelVram, utilisation: this.loaded.some((m) => m.state === 'busy') ? Math.round(62 + this.rnd() * 30) : Math.round(this.rnd() * 6), unified: false }],
      pitwallRss: 0.19 * GiB + modelRam + this.loaded.length * 0.11 * GiB,
      foregroundBusy: false,
    };
  }

  private demoQueue(now: number): QueueSnapshot {
    const running: Job[] = [
      { id: 'job-41', appId: 'imt', modelId: 'qwen3-8b-instruct-q4_k_m', priority: 'interactive', kind: 'chat', state: 'running', submittedAt: now - 3_400, startedAt: now - 3_100, progress: null, tokens: 212 },
      { id: 'job-39', appId: 'spotter', modelId: 'nomic-embed-text-v1.5-f16', priority: 'batch', kind: 'batch', state: 'running', submittedAt: now - 84_000, startedAt: now - 80_000, progress: 0.42, tokens: 182_400 },
    ];
    const waiting: Job[] = [
      { id: 'job-42', appId: 'continue', modelId: 'llama-3.2-3b-instruct-q5_k_m', priority: 'interactive', kind: 'completion', state: 'queued', submittedAt: now - 600, startedAt: null, progress: null, tokens: 0 },
      { id: 'job-43', appId: 'pitcrew', modelId: 'qwen3-8b-instruct-q4_k_m', priority: 'background', kind: 'task', state: 'queued', submittedAt: now - 12_000, startedAt: null, progress: null, tokens: 0 },
      { id: 'job-44', appId: 'spotter', modelId: 'bge-reranker-v2-m3-q8_0', priority: 'batch', kind: 'batch', state: 'queued', submittedAt: now - 30_000, startedAt: null, progress: 0, tokens: 0 },
    ];
    return { running, waiting, depth: this.depth(waiting) };
  }

  private depth(waiting: Job[]): QueueSnapshot['depth'] {
    return {
      interactive: waiting.filter((j) => j.priority === 'interactive').length,
      background: waiting.filter((j) => j.priority === 'background').length,
      batch: waiting.filter((j) => j.priority === 'batch').length,
    };
  }

  /** One round of live updates (the interval calls it every second; tests call it directly). */
  tick(): void {
    const now = this.now();
    this.ticks++;
    this.memory = this.sampleMemory(now);
    this.emit({ kind: 'memory', sample: clone(this.memory) });
    if (this.server.serving === 'serving' && this.loaded.length) {
      const busy = this.loaded.some((m) => m.state === 'busy');
      this.throughput = {
        promptTps: Math.round((busy ? 900 : 260) + this.rnd() * 500),
        genTps: Math.round(((busy ? 52 : 18) + this.rnd() * 14) * 10) / 10,
        ttftP50Ms: Math.round(190 + this.rnd() * 60),
        requestsPerMin: Math.round(30 + this.rnd() * 14),
        activeRequests: this.queue.running.length,
      };
    } else {
      this.throughput = { promptTps: 0, genTps: 0, ttftP50Ms: this.throughput.ttftP50Ms, requestsPerMin: 0, activeRequests: 0 };
    }
    this.emit({ kind: 'throughput', throughput: clone(this.throughput) });
    for (const job of this.queue.running) {
      job.tokens += Math.round(this.throughput.genTps);
      if (job.progress !== null) job.progress = Math.min(0.99, job.progress + 0.004);
    }
    if (this.ticks % 2 === 0) this.emit({ kind: 'queue', queue: clone(this.queue) });
    this.advanceDownloads();
    // Idle release: a model past its deadline is released, with an event the feed and the chart show.
    const due = this.loaded.find((m) => m.idleReleaseAt !== null && m.idleReleaseAt <= now);
    if (due) {
      this.loaded = this.loaded.filter((m) => m !== due);
      this.setLibraryLoaded(due.modelId, false);
      this.pushEvent({ kind: 'modelUnloaded', severity: 'info', message: `Released ${due.name}: idle for ${Math.round(this.settings_.memory.idleUnloadSecs / 60)} min`, modelId: due.modelId, appId: null, reason: 'idle' });
      this.emit({ kind: 'models', models: clone(this.loaded) });
    }
  }

  private advanceDownloads(): void {
    for (const d of this.downloads_) {
      if (d.state !== 'downloading' && d.state !== 'verifying' && d.state !== 'queued') continue;
      if (d.state === 'queued') d.state = 'downloading';
      if (d.state === 'verifying') {
        d.state = 'done';
        d.speedBps = 0;
        this.finishDownload(d);
      } else {
        d.speedBps = Math.round((32 + this.rnd() * 14) * MiB * (d.kind === 'engine' ? 0.6 : 1));
        d.bytesDone = Math.min(d.bytesTotal, d.bytesDone + d.speedBps * 8);
        if (d.bytesDone >= d.bytesTotal) d.state = 'verifying';
      }
      this.emit({ kind: 'download', download: clone(d) });
    }
  }

  private finishDownload(d: Download): void {
    const now = this.now();
    if (d.kind === 'model') {
      const quant = /-(Q\d\w*|F16|BF16)(?:-|\.)/i.exec(d.name)?.[1] ?? null;
      const id = d.name.replace(/\.gguf$/i, '').toLowerCase();
      if (!this.library.some((m) => m.id === id)) {
        this.library.push({ id, repo: d.host, file: d.name, name: d.name.replace(/\.gguf$/i, '').replace(/-/g, ' '), quant, sizeBytes: d.bytesTotal, paramsB: null, contextMax: null, architecture: null, capabilities: ['chat'], projector: null, addedAt: now, lastUsedAt: null, loaded: false, pinned: false, idleSecs: null, fit: demoFit(d.bytesTotal) });
      }
    } else {
      const engine = this.engineOfBuild(d.id.replace(/^eng-\d+-/, ''));
      const cat = engine?.catalog.find((c) => d.id.endsWith(c.id));
      if (engine && cat) {
        cat.installed = true;
        engine.builds = engine.builds.filter((b) => b.id !== cat.id);
        engine.builds.push({ id: cat.id, tag: cat.tag, label: cat.label, sizeBytes: cat.downloadBytes * 1.05, selfTest: 'passed', selfTestMessage: 'Self-test passed', devices: [engine.kind === 'pack' ? 'CPU' : GPU_NAME], active: engine.kind === 'pack', outdated: false });
        if (engine.kind === 'pack') engine.availability = { state: 'available' };
      }
    }
    this.pushEvent({ kind: 'download', severity: 'info', message: `Downloaded ${d.name} (${(d.bytesTotal / GiB).toFixed(1)} GB, verified)`, modelId: null, appId: null, reason: null });
  }

  private pushEvent(e: Omit<PitwallEvent, 'id' | 'at'>): void {
    const event: PitwallEvent = { ...e, id: 5000 + this.nextId++, at: this.now() };
    this.events_.unshift(event);
    this.events_ = this.events_.slice(0, 200);
    this.emit({ kind: 'event', event: clone(event) });
  }

  private emit(update: LiveUpdate): void {
    for (const l of [...this.listeners]) l(update);
  }

  private setLibraryLoaded(id: string, loaded: boolean): void {
    const m = this.library.find((x) => x.id === id);
    if (m) m.loaded = loaded;
  }

  private async reply<T>(method: keyof PitwallBackend, value: () => T): Promise<T> {
    if (this.latency) await new Promise((r) => setTimeout(r, this.latency));
    if (this.failing.has(method)) {
      const error: CommandError = { code: 'serverUnavailable', message: 'The Pitwall server is not responding. It may still be starting.' };
      throw error;
    }
    return clone(value());
  }

  private fail(code: CommandError['code'], message: string): never {
    const error: CommandError = { code, message };
    throw error;
  }

  // ---- Dashboard ---------------------------------------------------------------------------------------------

  overview(): Promise<Overview> {
    return this.reply('overview', () => ({
      server: this.server,
      memory: this.memory,
      models: this.loaded,
      engines: this.engines_.map((e) => ({
        kind: e.kind,
        name: e.name,
        availability: e.availability,
        activeBuild: e.builds.find((b) => b.active) ? `${e.builds.find((b) => b.active)!.id} · ${e.builds.find((b) => b.active)!.tag}` : null,
        processes: e.processes.length,
        unhealthy: e.processes.filter((p) => p.health === 'unhealthy' || p.health === 'restarting').length,
      })),
      queue: this.queue,
      throughput: this.throughput,
      apps: this.apps_,
      preset: this.settings_.memory.preset,
      pairings: this.pairings_,
      issues: this.issues_,
    }));
  }

  history(range: HistoryRange): Promise<History> {
    return this.reply('history', () => {
      let h = this.histories.get(range);
      if (!h) {
        h = demoHistory(range, this.now());
        this.histories.set(range, h);
      }
      return h;
    });
  }

  events(limit: number): Promise<PitwallEvent[]> {
    return this.reply('events', () => this.events_.slice(0, limit));
  }

  subscribe(listener: (update: LiveUpdate) => void): Unsubscribe {
    this.listeners.add(listener);
    if (this.autoTick && !this.timer) this.timer = setInterval(() => this.tick(), 1000);
    return () => {
      this.listeners.delete(listener);
      if (!this.listeners.size && this.timer) {
        clearInterval(this.timer);
        this.timer = null;
      }
    };
  }

  setServing(paused: boolean): Promise<void> {
    return this.reply('setServing', () => {
      this.server.serving = paused ? 'paused' : 'serving';
      this.emit({ kind: 'server', server: clone(this.server) });
      this.pushEvent({ kind: 'servingChanged', severity: 'info', message: paused ? 'Serving paused: new requests wait until you resume' : 'Serving resumed', modelId: null, appId: null, reason: null });
    });
  }

  releaseMemory(): Promise<number> {
    return this.reply('releaseMemory', () => {
      const released = this.loaded.filter((m) => !m.pinned && m.activeRequests === 0);
      const freed = released.reduce((s, m) => s + m.ramBytes + m.vramBytes, 0);
      this.loaded = this.loaded.filter((m) => !released.includes(m));
      for (const m of released) {
        this.setLibraryLoaded(m.modelId, false);
        this.pushEvent({ kind: 'modelUnloaded', severity: 'info', message: `Released ${m.name}: released by you`, modelId: m.modelId, appId: null, reason: 'manual' });
      }
      this.emit({ kind: 'models', models: clone(this.loaded) });
      return freed;
    });
  }

  applyPreset(preset: Preset): Promise<Settings> {
    return this.reply('applyPreset', () => {
      this.settings_.memory = presetMemory(preset, this.settings_.memory);
      this.pushEvent({ kind: 'presetChanged', severity: 'info', message: `Memory preset set to ${preset === 'lowMemory' ? 'Low memory' : preset[0]!.toUpperCase() + preset.slice(1)}`, modelId: null, appId: null, reason: null });
      return this.settings_;
    });
  }

  cancelJob(id: string): Promise<void> {
    return this.reply('cancelJob', () => {
      const before = this.queue.running.length + this.queue.waiting.length;
      this.queue.running = this.queue.running.filter((j) => j.id !== id);
      this.queue.waiting = this.queue.waiting.filter((j) => j.id !== id);
      if (this.queue.running.length + this.queue.waiting.length === before) this.fail('notFound', `No job ${id}.`);
      this.queue.depth = this.depth(this.queue.waiting);
      this.emit({ kind: 'queue', queue: clone(this.queue) });
    });
  }

  // ---- Models ------------------------------------------------------------------------------------------------

  localModels(): Promise<LocalModel[]> {
    return this.reply('localModels', () => this.library);
  }

  searchModels(query: string): Promise<HfModel[]> {
    return this.reply('searchModels', () => {
      if (this.server.offline) this.fail('offline', 'Offline mode is on: Pitwall makes no network requests. Turn it off in Settings ▸ Network.');
      return demoSearch(query, this.now());
    });
  }

  repoFiles(repo: string): Promise<HfRepo> {
    return this.reply('repoFiles', () => demoRepo(repo, this.library));
  }

  downloadModel(repo: string, file: string, projector: string | null): Promise<Download> {
    return this.reply('downloadModel', () => {
      const variant = demoRepo(repo, this.library).variants.find((v) => v.file === file);
      const d: Download = { id: `dl-${10 + this.nextId++}`, kind: 'model', name: file, host: 'huggingface.co', bytesDone: 0, bytesTotal: (variant?.sizeBytes ?? 2 * GiB) + (projector ? 854 * MiB : 0), speedBps: 0, state: 'queued', error: null, startedAt: this.now() };
      this.downloads_.unshift(d);
      this.emit({ kind: 'download', download: clone(d) });
      return d;
    });
  }

  downloads(): Promise<Download[]> {
    return this.reply('downloads', () => this.downloads_);
  }

  cancelDownload(id: string): Promise<void> {
    return this.reply('cancelDownload', () => {
      const d = this.downloads_.find((x) => x.id === id) ?? this.fail('notFound', `No download ${id}.`);
      d.state = 'cancelled';
      d.speedBps = 0;
      this.emit({ kind: 'download', download: clone(d) });
    });
  }

  deleteModel(id: string): Promise<void> {
    return this.reply('deleteModel', () => {
      if (this.loaded.some((m) => m.modelId === id)) this.fail('conflict', 'Unload the model before deleting it.');
      this.library = this.library.filter((m) => m.id !== id);
    });
  }

  loadModel(id: string): Promise<void> {
    return this.reply('loadModel', () => {
      const m = this.library.find((x) => x.id === id) ?? this.fail('notFound', `No model ${id}.`);
      if (m.fit.verdict === 'wontFit') this.fail('insufficientMemory', `${m.name} needs more memory than this machine has free.`);
      if (this.issues_.some((i) => i.modelId === id)) {
        this.issues_ = this.issues_.filter((i) => i.modelId !== id);
        this.emit({ kind: 'issues', issues: clone(this.issues_) });
      }
      if (!this.loaded.some((x) => x.modelId === id)) {
        const now = this.now();
        const vram = Math.min(m.sizeBytes * 0.9, 3.5 * GiB);
        this.loaded.push({ modelId: id, name: m.name, engine: 'llama.cpp', build: 'cuda-13 · b11669', state: 'ready', ramBytes: m.sizeBytes * 1.1 - vram, vramBytes: vram, contextTokens: 8192, pinned: m.pinned, activeRequests: 0, loadedAt: now, lastUsedAt: now, idleReleaseAt: m.pinned ? null : now + (m.idleSecs ?? this.settings_.memory.idleUnloadSecs) * 1000, coldStartMs: 2100, capabilities: m.capabilities });
        m.loaded = true;
        this.pushEvent({ kind: 'modelLoaded', severity: 'info', message: `Loaded ${m.name} (2.1 s cold start)`, modelId: id, appId: null, reason: null });
        this.emit({ kind: 'models', models: clone(this.loaded) });
      }
    });
  }

  unloadModel(id: string): Promise<void> {
    return this.reply('unloadModel', () => {
      const m = this.loaded.find((x) => x.modelId === id) ?? this.fail('notFound', 'That model is not loaded.');
      this.loaded = this.loaded.filter((x) => x !== m);
      this.setLibraryLoaded(id, false);
      this.pushEvent({ kind: 'modelUnloaded', severity: 'info', message: `Released ${m.name}: released by you`, modelId: id, appId: null, reason: 'manual' });
      this.emit({ kind: 'models', models: clone(this.loaded) });
    });
  }

  pinModel(id: string, pinned: boolean): Promise<void> {
    return this.reply('pinModel', () => {
      const lib = this.library.find((x) => x.id === id) ?? this.fail('notFound', `No model ${id}.`);
      lib.pinned = pinned;
      const hot = this.loaded.find((x) => x.modelId === id);
      if (hot) {
        hot.pinned = pinned;
        hot.idleReleaseAt = pinned || hot.activeRequests ? null : this.now() + (lib.idleSecs ?? this.settings_.memory.idleUnloadSecs) * 1000;
        this.emit({ kind: 'models', models: clone(this.loaded) });
      }
    });
  }

  setModelIdle(id: string, idleSecs: number | null): Promise<void> {
    return this.reply('setModelIdle', () => {
      if (idleSecs !== null && (!Number.isInteger(idleSecs) || idleSecs < 30)) this.fail('badRequest', 'The idle time must be at least 30 seconds.');
      const lib = this.library.find((x) => x.id === id) ?? this.fail('notFound', `No model ${id}.`);
      lib.idleSecs = idleSecs;
    });
  }

  diskUsage(): Promise<DiskUsage> {
    return this.reply('diskUsage', () => {
      const now = this.now();
      return {
        modelsBytes: this.library.reduce((s, m) => s + m.sizeBytes + (m.projector ? 854 * MiB : 0), 0),
        enginesBytes: this.engines_[0]!.builds.reduce((s, b) => s + b.sizeBytes, 0),
        partialBytes: this.downloads_.filter((d) => d.state !== 'done' && d.state !== 'cancelled').reduce((s, d) => s + d.bytesDone, 0),
        freeBytes: 312 * GiB,
        unused: this.library.filter((m) => !m.loaded && (m.lastUsedAt === null || now - m.lastUsedAt > 30 * 86_400_000)).map((m) => m.id),
        unusedAfterDays: 30,
      };
    });
  }

  // ---- Engines -----------------------------------------------------------------------------------------------

  engines(): Promise<Engine[]> {
    return this.reply('engines', () => this.engines_);
  }

  private engineOfBuild(buildId: string): Engine | undefined {
    return this.engines_.find((e) => e.catalog.some((c) => c.id === buildId));
  }

  packs(tag: string | null = null): Promise<PacksInfo> {
    return this.reply('packs', () => demoPacks(tag));
  }

  setModelEngine(id: string, engine: string | null): Promise<void> {
    return this.reply('setModelEngine', () => {
      const m = this.library.find((x) => x.id === id) ?? this.fail('notFound', `No model ${id}.`);
      if (engine !== null && engine !== 'llama.cpp' && !this.engines_.some((e) => e.kind === 'pack' && e.catalog.some((c) => c.id === engine))) {
        this.fail('notFound', `There is no engine pack "${engine}".`);
      }
      m.engine = engine === 'llama.cpp' ? null : engine;
    });
  }

  installBuild(buildId: string): Promise<Download> {
    return this.reply('installBuild', () => {
      if (this.server.offline) this.fail('offline', 'Offline mode is on: Pitwall downloads nothing.');
      const engine = this.engineOfBuild(buildId) ?? this.fail('notFound', `No build ${buildId} in the catalog.`);
      const cat = engine.catalog.find((c) => c.id === buildId)!;
      const d: Download = { id: `eng-${this.nextId++}-${cat.id}`, kind: 'engine', name: `${engine.name} ${cat.label} ${cat.tag}`, host: 'github.com', bytesDone: 0, bytesTotal: cat.downloadBytes, speedBps: 0, state: 'queued', error: null, startedAt: this.now() };
      this.downloads_.unshift(d);
      this.emit({ kind: 'download', download: clone(d) });
      return d;
    });
  }

  removeBuild(buildId: string, tag: string): Promise<void> {
    return this.reply('removeBuild', () => {
      const e = this.engines_[0]!;
      const b = e.builds.find((x) => x.id === buildId && x.tag === tag) ?? this.fail('notFound', 'No such build.');
      if (b.active) this.fail('conflict', 'This build is in use. Choose another build first.');
      e.builds = e.builds.filter((x) => x !== b);
      const cat = e.catalog.find((c) => c.id === buildId && c.tag === tag);
      if (cat) cat.installed = false;
    });
  }

  selectBuild(choice: string): Promise<void> {
    return this.reply('selectBuild', () => {
      const e = this.engines_[0]!;
      if (choice !== 'auto' && !e.builds.some((b) => b.id === choice)) this.fail('notFound', `No installed build ${choice}.`);
      e.buildChoice = choice;
      const pick = choice === 'auto' ? (e.builds.find((b) => b.selfTest === 'passed' && b.id.startsWith('cuda')) ?? e.builds[0]) : e.builds.find((b) => b.id === choice);
      for (const b of e.builds) b.active = b === pick;
    });
  }

  rollbackBuild(): Promise<void> {
    return this.reply('rollbackBuild', () => {
      const e = this.engines_[0]!;
      if (!e.rollbackTo) this.fail('conflict', 'There is nothing to roll back to.');
      e.rollbackTo = null;
    });
  }

  // ---- Apps --------------------------------------------------------------------------------------------------

  apps(): Promise<AppInfo[]> {
    return this.reply('apps', () => this.apps_);
  }

  createApp(name: string): Promise<AppInfo> {
    return this.reply('createApp', () => {
      const n = name.trim();
      if (!n) this.fail('badRequest', 'Give the app a name.');
      const id = n.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '') || `app-${this.nextId++}`;
      if (this.apps_.some((a) => a.id === id)) this.fail('conflict', `An app named ${n} exists already.`);
      const app: AppInfo = {
        id,
        name: n,
        suite: false,
        connected: false,
        lastSeenAt: null,
        keys: [],
        permissions: { models: null, maxPriority: 'background', tools: false, extendedApis: false, limits: { requestsPerMin: 60, tokensPerDay: null, maxConcurrent: 1 } },
        today: { requests: 0, promptTokens: 0, completionTokens: 0, busyMs: 0, errors: 0 },
      };
      this.apps_.push(app);
      return app;
    });
  }

  deleteApp(id: string): Promise<void> {
    return this.reply('deleteApp', () => {
      this.apps_ = this.apps_.filter((a) => a.id !== id);
    });
  }

  createKey(appId: string): Promise<CreatedKey> {
    return this.reply('createKey', () => {
      const app = this.apps_.find((a) => a.id === appId) ?? this.fail('notFound', `No app ${appId}.`);
      const hex = Array.from({ length: 40 }, () => Math.floor(this.rnd() * 16).toString(16)).join('');
      const secret = `pw_${hex}`;
      const key = { id: `${appId}-k${this.nextId++}`, prefix: `${secret.slice(0, 7)}…`, createdAt: this.now(), lastUsedAt: null };
      app.keys.push(key);
      return { appId, key, secret };
    });
  }

  revokeKey(appId: string, keyId: string): Promise<void> {
    return this.reply('revokeKey', () => {
      const app = this.apps_.find((a) => a.id === appId) ?? this.fail('notFound', `No app ${appId}.`);
      app.keys = app.keys.filter((k) => k.id !== keyId);
    });
  }

  setPermissions(appId: string, permissions: AppPermissions): Promise<AppInfo> {
    return this.reply('setPermissions', () => {
      const app = this.apps_.find((a) => a.id === appId) ?? this.fail('notFound', `No app ${appId}.`);
      app.permissions = clone(permissions);
      return app;
    });
  }

  approvePairing(id: string, permissions: AppPermissions): Promise<void> {
    return this.reply('approvePairing', () => {
      const p = this.pairings_.find((x) => x.id === id) ?? this.fail('notFound', 'That pairing request has expired.');
      this.pairings_ = this.pairings_.filter((x) => x !== p);
      if (!this.apps_.some((a) => a.id === p.appId)) {
        this.apps_.push({ id: p.appId, name: p.appName, suite: false, connected: true, lastSeenAt: this.now(), keys: [{ id: `${p.appId}-k1`, prefix: 'pw_8c1e…', createdAt: this.now(), lastUsedAt: null }], permissions: clone(permissions), today: { requests: 0, promptTokens: 0, completionTokens: 0, busyMs: 0, errors: 0 } });
      }
      this.pushEvent({ kind: 'pairing', severity: 'info', message: `Paired ${p.appName}`, modelId: null, appId: p.appId, reason: null });
      this.emit({ kind: 'pairing', pairings: clone(this.pairings_) });
    });
  }

  denyPairing(id: string): Promise<void> {
    return this.reply('denyPairing', () => {
      const p = this.pairings_.find((x) => x.id === id);
      this.pairings_ = this.pairings_.filter((x) => x.id !== id);
      if (p) this.pushEvent({ kind: 'pairing', severity: 'info', message: `Denied pairing for ${p.appName}`, modelId: null, appId: p.appId, reason: null });
      this.emit({ kind: 'pairing', pairings: clone(this.pairings_) });
    });
  }

  pairings(): Promise<PairingRequest[]> {
    return this.reply('pairings', () => this.pairings_);
  }

  // ---- Requests ----------------------------------------------------------------------------------------------

  requests(limit: number): Promise<RequestRecord[]> {
    return this.reply('requests', () => this.requests_.slice(0, limit));
  }

  requestContent(id: string): Promise<RequestContent> {
    return this.reply('requestContent', () => {
      const r = this.requests_.find((x) => x.id === id) ?? this.fail('notFound', `No request ${id}.`);
      if (!r.hasContent) this.fail('notFound', 'Pitwall did not keep the content of this request (Settings ▸ Privacy).');
      const request = JSON.stringify(
        { model: r.modelId, stream: r.stream, messages: [{ role: 'system', content: 'You summarise log excerpts for an engineer.' }, { role: 'user', content: 'Summarise these 40 lines: service restarted twice after a timeout in the connection pool…' }] },
        null,
        2,
      );
      const response = JSON.stringify({ choices: [{ message: { role: 'assistant', content: 'The service restarted twice because the connection pool timed out after 30 s while the database was failing over.' } }], usage: { prompt_tokens: r.promptTokens, completion_tokens: r.completionTokens } }, null, 2);
      return { id, request, response };
    });
  }

  // ---- Playground and benchmarks -----------------------------------------------------------------------------

  chat(request: PlaygroundRequest, onEvent: (event: PlaygroundEvent) => void): Unsubscribe {
    let stopped = false;
    const timers: ReturnType<typeof setTimeout>[] = [];
    const words = (request.modelId.includes('llama') ? DEMO_REPLY.split(' ').slice(0, 38).join(' ') + '.' : DEMO_REPLY).split(/(?<= )/);
    const limit = Math.max(1, Math.min(words.length, Math.ceil(request.maxTokens / 1.3)));
    const run = () => {
      if (this.failing.has('chat')) {
        onEvent({ kind: 'error', error: { code: 'serverUnavailable', message: 'The Pitwall server is not responding.' } });
        return;
      }
      if (!this.library.some((m) => m.id === request.modelId)) {
        onEvent({ kind: 'error', error: { code: 'notFound', message: `No model ${request.modelId}.` } });
        return;
      }
      let i = 0;
      const step = () => {
        if (stopped) return;
        if (i < limit) {
          onEvent({ kind: 'delta', text: words[i++]! });
          timers.push(setTimeout(step, this.streamDelay));
        } else {
          const fast = request.modelId.includes('llama');
          onEvent({ kind: 'done', promptTokens: 18 + request.messages.reduce((s, m) => s + Math.ceil(m.content.length / 4), 0), completionTokens: Math.round(limit * 1.3), ttftMs: fast ? 96 : 214, genTps: fast ? 118.4 : 61.8, coldStartMs: this.loaded.some((m) => m.modelId === request.modelId) ? null : 2100 });
        }
      };
      timers.push(setTimeout(step, this.streamDelay ? this.streamDelay * 4 : 0));
    };
    timers.push(setTimeout(run, 0));
    return () => {
      stopped = true;
      timers.forEach(clearTimeout);
    };
  }

  benchmarks(): Promise<BenchmarkResult[]> {
    return this.reply('benchmarks', () => this.benchmarks_);
  }

  runBenchmark(modelId: string): Promise<BenchmarkResult> {
    return this.reply('runBenchmark', () => {
      const m = this.library.find((x) => x.id === modelId) ?? this.fail('notFound', `No model ${modelId}.`);
      const scale = 8 / Math.max(0.5, m.paramsB ?? 4);
      const result: BenchmarkResult = { modelId, build: 'cuda-13 · b11669', at: this.now(), promptTps: Math.round(1600 * scale + this.rnd() * 200), genTps: Math.round((52 * Math.sqrt(scale) + this.rnd() * 4) * 10) / 10, ttftMs: Math.round(240 / Math.sqrt(scale)), contextTokens: 4096 };
      this.benchmarks_.unshift(result);
      return result;
    });
  }

  // ---- Settings ------------------------------------------------------------------------------

  settings(): Promise<Settings> {
    return this.reply('settings', () => this.settings_);
  }

  saveSettings(settings: Settings): Promise<Settings> {
    return this.reply('saveSettings', () => {
      if (settings.server.port < 1024 || settings.server.port > 65535) this.fail('badRequest', 'The port must be between 1024 and 65535.');
      this.settings_ = clone(settings);
      this.server.offline = settings.network.offline;
      this.server.port = settings.server.port;
      this.server.lan = settings.network.lanEnabled
        ? { enabled: true, url: `https://192.168.1.42:${settings.network.lanPort}`, certFingerprint: '4F:9A:21:C0:7E:55:B3:18:9D:02:6A:F1:3C:88:E4:57:0B:A9:D6:12:7F:C3:45:8E:91:2D:B0:66:EA:73:19:5C' }
        : { enabled: false, url: null, certFingerprint: null };
      this.emit({ kind: 'server', server: clone(this.server) });
      return this.settings_;
    });
  }

  appUpdateStatus(): Promise<AppUpdateStatus> {
    return this.reply('appUpdateStatus', () =>
      this.updateAvailable
        ? { available: true, reason: null, currentVersion: this.server.version }
        : { available: false, reason: 'Updates are not configured for this build (no updater key).', currentVersion: this.server.version },
    );
  }

  checkAppUpdate(): Promise<UpdateInfo | null> {
    return this.reply('checkAppUpdate', () => {
      if (!this.updateAvailable) this.fail('notAvailable', 'Updates are not available in this build.');
      return { version: '0.2.0', notes: 'Faster model loading and a new dashboard tile.' };
    });
  }

  installAppUpdate(): Promise<void> {
    return this.reply('installAppUpdate', () => {
      if (!this.updateAvailable) this.fail('notAvailable', 'Updates are not available in this build.');
    });
  }

  serviceState(): Promise<ServiceInstall> {
    return this.reply('serviceState', () => this.service);
  }

  installService(): Promise<ServiceInstall> {
    return this.reply('installService', () => {
      this.service = { ...this.service, installed: true, running: true, askConsent: false, message: null };
      return this.service;
    });
  }

  uninstallService(): Promise<ServiceInstall> {
    return this.reply('uninstallService', () => {
      this.service = { ...this.service, installed: false, askConsent: false };
      return this.service;
    });
  }

  declineService(): Promise<void> {
    return this.reply('declineService', () => {
      this.service = { ...this.service, askConsent: false };
    });
  }

  autostart(): Promise<boolean> {
    return this.reply('autostart', () => this.autostart_);
  }

  setAutostart(enabled: boolean): Promise<boolean> {
    return this.reply('setAutostart', () => {
      this.autostart_ = enabled;
      return enabled;
    });
  }
}
