import { buildDownloadName } from '../shared/build-download';
import type {
  AppDetail,
  ChatModels,
  ChatStorage,
  OpenedApp,
  ConnectedApp,
  ModelPreset,
  PresetDraft,
  PresetEntry,
  PresetPreview,
  PresetStatus,
  AppInfo,
  CacheClearScope,
  CacheInfo,
  AppPermissions,
  BenchmarkResult,
  BuildCapabilities,
  CommandError,
  Fit,
  CreatedKey,
  DiskUsage,
  SlotFit,
  SlotsChange,
  Download,
  Engine,
  EngineIssue,
  EngineSetup,
  EnginePinState,
  PinChange,
  RollbackTarget,
  History,
  UsageRange,
  UsageSeries,
  HistoryRange,
  HfModel,
  HfRepo,
  Job,
  JobEvent,
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
  Platform,
  Preset,
  QueueSnapshot,
  RequestContent,
  RequestRecord,
  ServiceInstall,
  UpdateCheckResult,
  UpdateCheckSettings,
  UpdateCheckStatus,
  UpdateNotice,
  UpdateSchedule,
  ServerInfo,
  Settings,
  Throughput,
} from './api.types';
import { BROWSER_VERSION } from '../shell/engines';
import type { PitwallBackend, Unsubscribe } from './backend';
import type { LoadingRows, PopoverHeader } from './generated/widgets';
import { LOADING_SHAPE, MockPopover, type PopoverScenario } from './mock/mock-popover';
import type { PopoverEdit, PopoverPush, PopoverValues, PopoverView } from './widgets.types';
import {
  demoReplyWords,
  wordsThatFit,
  GPU_NAME,
  GiB,
  MiB,
  RAM_TOTAL,
  VRAM_TOTAL,
  demoApps,
  demoBenchmarks,
  demoBuildCapabilities,
  demoEngines,
  demoEvents,
  demoFit,
  demoAppDetail,
  demoHistory,
  demoUsageSeries,
  demoLoaded,
  demoLocalModels,
  demoPacks,
  demoPairings,
  demoPresets,
  demoRepo,
  demoSlotFit,
  demoRequests,
  demoSearch,
  demoSettings,
  demoUpdateNotices,
  presetMemory,
  prng,
} from './mock-data';

/** The slots a model loads with unless it has its own choice, and the context each slot gets (mirrors the engine's defaults). */
const DEFAULT_SLOTS = 2;
const DEFAULT_CONTEXT = 8192;

/** What each job method is called while it runs (the real titles come from the app, `app_commands.rs`). */
const JOB_TITLES: Partial<Record<keyof PitwallBackend, string>> = {
  loadModel: 'Loading the model',
  runBenchmark: 'Running the benchmark',
  checkUpdatesNow: 'Checking for model updates',
  installService: 'Installing the service',
  uninstallService: 'Removing the service',
  repairService: 'Repairing the service',
};

/** The calls the desktop app answers itself, without the server (they keep working when the server is down). */
const APP_METHODS: ReadonlySet<string> = new Set([
  'serviceState', 'installService', 'uninstallService', 'repairService', 'declineService', 'openLoginItems',
  'autostart', 'setAutostart', 'quitApp', 'takeQuitAsk', 'chatEnabled',
  'connectedApps', 'setAppKey', 'connectApp', 'forgetApp', 'revokeToolApproval', 'openConnectedApp',
  'chatStorage', 'setChatSaveNew', 'chatSaved', 'setChatSaved', 'deleteAllChats',
  // The popup's widgets are the app's own too (its layout is kept by the app; the values say "server not running" themselves).
  'popoverView', 'popoverValues', 'popoverHeader', 'popoverShape', 'editPopover',
]);

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
  /** The OS the service is registered with (its wording in Settings and the first-run question); default macOS. */
  platform?: Platform;
  /** macOS: the background item is switched off in Login Items (notice with an "Open Login Items" button). */
  loginItemOff?: boolean;
  /** The server is down: every call except the app's own (service, log-in, updates) fails with `serverUnavailable`. */
  serverDown?: boolean;
  /** macOS: launchd refuses the service after an update (the "could not start" state with Repair). */
  startFailed?: boolean;
  /** macOS: a repair of a refused registration is under way (notice with its progress). */
  repairing?: boolean;
  /** A model's engine crashed repeatedly and was given up on (dashboard issue with Retry). */
  crashed?: boolean;
  /** Model update notices are already on offer (otherwise "Check now" finds them). */
  modelUpdates?: boolean;
  /** Delay between streamed playground words (ms). */
  streamDelayMs?: number;
  /** llama.cpp is installing itself (progress banner on the dashboard and the llama.cpp page). */
  installing?: boolean;
  /** No file from an older version is left on disk (a clean install; the default demo offers a leftover llamafile). */
  noLeftovers?: boolean;
  /** The app asked to quit with "Ask each time" set (the question dialog opens). */
  quitAsk?: boolean;
  /** This build has the Playground's chat (default: yes; `?mock-nochat` plays a build without it). */
  chat?: boolean;
  /** What the tray popup's widgets show: the engine's recorded output for a scenario (default: `demo`, or `empty` / `down` with those flags). */
  popover?: PopoverScenario;
}

const clone = <T>(v: T): T => structuredClone(v);

/**
 * The browser-build backend: realistic, deterministic demo data and simulated live updates. Browser development and
 * UI tests only; it mimics the contract and is never the reference for behaviour.
 */
const CAPABILITY_WORDS = ['chat', 'vision', 'tools', 'json', 'embeddings', 'rerank', 'transcription'];
const PREFER_WORDS = ['fast', 'quality', 'small', 'balanced'];

export class MockBackend implements PitwallBackend {
  private readonly now: () => number;
  private readonly latency: number;
  private readonly autoTick: boolean;
  private readonly failing: ReadonlySet<string>;
  private serverDown = false;
  private readonly popover_ = new MockPopover();
  private readonly popoverScenario: PopoverScenario;
  private readonly popoverListeners = new Set<(push: PopoverPush) => void>();
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
  private pins_: { global: string | null; models: Record<string, string>; history: string[] } = { global: null, models: {}, history: ['vulkan'] };
  private downloads_: Download[] = [];
  private readonly requests_: RequestRecord[];
  private presets_: ModelPreset[];
  private readonly benchmarks_: BenchmarkResult[];
  private readonly histories = new Map<HistoryRange, History>();
  private readonly listeners = new Set<(u: LiveUpdate) => void>();
  private timer: ReturnType<typeof setInterval> | null = null;
  private ticks = 0;
  private nextId = 1;
  private service: ServiceInstall;
  private autostart_ = false;
  private issues_: EngineIssue[] = [];
  private engineSetup_: EngineSetup = { state: 'idle', error: null, download: null };
  private updateCheck_: UpdateCheckSettings = { enabled: false, schedule: 'weekly' };
  private updateNotices_: UpdateNotice[] = [];
  private updateHidden_ = new Set<string>();
  private cache_: { responses: number; embeddings: number } = { responses: 184, embeddings: 3412 };
  private updateChecked_: { at: number; message: string } | null = null;
  private readonly chat_: boolean;

  constructor(options: MockOptions = {}) {
    this.now = options.now ?? (() => Date.now());
    this.latency = options.latencyMs ?? 0;
    this.autoTick = options.autoTick ?? false;
    this.failing = new Set(options.failing ?? []);
    this.rnd = prng(options.seed ?? 42);
    this.streamDelay = options.streamDelayMs ?? 0;
    this.popoverScenario = options.popover ?? (options.serverDown ? 'down' : options.empty ? 'empty' : 'demo');
    if (options.modelUpdates) this.updateNotices_ = demoUpdateNotices(this.now());
    if (options.quitAsk) this.quitAskPending = true;
    this.chat_ = options.chat ?? true;
    if (options.installing) {
      // The catalog's suggested build (cuda-13 at its pinned release, its download size, named as the server names it): the first
      // start installs exactly that.
      const download: Download = { id: 'eng-0', kind: 'engine', name: 'llama.cpp CUDA 13 (NVIDIA) (b11702)', host: 'github.com', bytesDone: 212 * MiB, bytesTotal: 398 * MiB, speedBps: 24 * MiB, state: 'downloading', error: null, startedAt: this.now() - 9_000 };
      this.engineSetup_ = { state: 'installing', error: null, download };
    }
    if (options.crashed) {
      this.issues_ = [
        { modelId: 'gemma-4-12b-it-q4_k_m', state: 'failed', crashes: 6, restartAt: null, message: 'The engine for gemma-4-12b-it-q4_k_m crashed repeatedly and Pitwall stopped restarting it. Check that the model file is complete, then press Retry.' },
      ];
    }
    this.service = {
      platform: options.platform ?? 'macos',
      installed: !options.askConsent,
      running: !(options.serverDown || options.startFailed),
      canInstall: true,
      askConsent: options.askConsent ?? false,
      message: options.startFailed ? 'launchd refused to start the service (error 78, bad configuration): the app was updated and macOS still holds the previous signature.' : null,
      repair: options.repairing ? { state: 'working', message: 'Repairing the background service: re-registering it with macOS… (this can take a minute)' } : null,
      disabledInLoginItems: options.loginItemOff ?? false,
      startFailed: options.startFailed ?? false,
      notInLoginItems: options.startFailed ?? false,
    };
    this.serverDown = options.serverDown ?? false;
    const now = this.now();
    const empty = options.empty ?? false;
    this.settings_ = demoSettings();
    this.server = {
      // The browser build's one version (the shell's About and Updates read the same constant), never a second literal.
      version: BROWSER_VERSION,
      apiVersion: 1,
      pid: 7412,
      port: this.settings_.server.port,
      startedAt: now - 5 * 3_600_000 - 12 * 60_000,
      serving: 'serving',
      lan: { enabled: false, url: null, certFingerprint: null },
    };
    this.library = empty ? [] : demoLocalModels(now);
    this.loaded = empty ? [] : demoLoaded(now, this.settings_.memory.idleUnloadSecs);
    this.apps_ = empty ? [] : demoApps(now);
    this.pairings_ = empty ? [] : demoPairings(now);
    this.events_ = empty ? [] : demoEvents(now);
    this.engines_ = demoEngines(now);
    if (options.noLeftovers) this.engines_.forEach((e) => (e.leftovers = []));
    // First start: no build is installed yet and nothing runs.
    if (options.installing) this.engines_.forEach((e) => Object.assign(e, { builds: [], processes: [], rollbackTo: null, availability: { state: 'notInstalled', reason: 'llama.cpp is being installed' } }));
    this.requests_ = empty ? [] : demoRequests(now);
    this.presets_ = empty ? [] : demoPresets();
    this.benchmarks_ = empty ? [] : demoBenchmarks(now);
    this.memory = this.sampleMemory(now);
    this.queue = empty ? { running: [], waiting: [], depth: { interactive: 0, background: 0, batch: 0 } } : this.demoQueue(now);
    this.throughput = empty ? { promptTps: 0, genTps: 0, ttftP50Ms: 0, requestsPerMin: 0, activeRequests: 0 } : { promptTps: 1184, genTps: 58.4, ttftP50Ms: 214, requestsPerMin: 37, activeRequests: 3 };
    if (empty) for (const r of ['1h', '24h', '7d', '30d'] as const) this.histories.set(r, { range: r, stepMs: 60_000, points: [], requestsByApp: [], requestsByModel: [], events: [] });
    if (!empty) {
      this.downloads_ = [
        { id: 'dl-1', kind: 'model', name: 'gemma-4-E4B-it-Q8_0.gguf', host: 'huggingface.co', bytesDone: 3.1 * GiB, bytesTotal: 7.63 * GiB, speedBps: 38 * MiB, state: 'downloading', error: null, startedAt: now - 90_000 },
        { id: 'dl-0', kind: 'model', name: 'gemma-4-26B-A4B-it-Q4_K_M.gguf', host: 'huggingface.co', bytesDone: 6.2 * GiB, bytesTotal: 15.4 * GiB, speedBps: 0, state: 'paused', error: 'Connection reset; resumes from 6.2 GB', startedAt: now - 3_600_000 },
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
      { id: 'job-41', appId: 'debrief', modelId: 'qwen3.5-9b-q4_k_m', priority: 'interactive', kind: 'chat', state: 'running', submittedAt: now - 3_400, startedAt: now - 3_100, progress: null, tokens: 212 },
      { id: 'job-39', appId: 'spotter', modelId: 'qwen3-embedding-0.6b-q8_0', priority: 'batch', kind: 'batch', state: 'running', submittedAt: now - 84_000, startedAt: now - 80_000, progress: 0.42, tokens: 182_400 },
    ];
    const waiting: Job[] = [
      { id: 'job-42', appId: 'nightly-report', modelId: 'qwen3.5-4b-q4_k_m', priority: 'interactive', kind: 'completion', state: 'queued', submittedAt: now - 600, startedAt: null, progress: null, tokens: 0 },
      { id: 'job-43', appId: 'pitcrew', modelId: 'qwen3.5-9b-q4_k_m', priority: 'background', kind: 'task', state: 'queued', submittedAt: now - 12_000, startedAt: null, progress: null, tokens: 0 },
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
        this.library.push({ id, repo: d.host, file: d.name, name: d.name.replace(/\.gguf$/i, '').replace(/-/g, ' '), quant, sizeBytes: d.bytesTotal, paramsB: null, contextMax: null, architecture: null, capabilities: ['chat'], projector: null, addedAt: now, lastUsedAt: null, loaded: false, pinned: false, idleSecs: null, slots: DEFAULT_SLOTS, slotsCustom: false, loadedSlots: null, fit: demoFit(d.bytesTotal) });
      }
    } else {
      const engine = this.engineOfBuild(d.id.replace(/^eng-\d+-/, ''));
      const cat = engine?.catalog.find((c) => d.id.endsWith(c.id));
      if (engine && cat) {
        cat.installed = true;
        engine.builds = engine.builds.filter((b) => b.id !== cat.id);
        engine.builds.push({ id: cat.id, tag: cat.tag, label: cat.label, sizeBytes: cat.downloadBytes * 1.05, selfTest: 'passed', selfTestMessage: 'Self-test passed', devices: [GPU_NAME], active: false, outdated: false });
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
    if (m) {
      m.loaded = loaded;
      m.loadedSlots = loaded ? m.slots : null;
    }
  }

  // ---- Jobs (the real backend runs these as background jobs, ADR 0024) -------------------------------------------

  /** Tests: jobs wait until cancelled (or stopped waiting) instead of finishing. */
  holdJobs = false;
  /** Tests: Cancel is refused, as for a step that cannot be interrupted; the job then offers "Stop waiting". */
  refuseCancel = false;
  private jobSeq = 0;
  private readonly jobListeners = new Set<(event: JobEvent) => void>();
  private readonly held = new Map<number, { end: (stoppedWaiting: boolean) => void; canStop: () => boolean }>();

  jobEvents(listener: (event: JobEvent) => void): Unsubscribe {
    this.jobListeners.add(listener);
    return () => void this.jobListeners.delete(listener);
  }

  jobCancel(jobId: number): Promise<boolean> {
    const job = this.held.get(jobId);
    if (!job) return Promise.resolve(false);
    if (this.refuseCancel) {
      this.emitJob({ kind: 'progress', jobId, phase: 'Cancel did not take effect', done: 1, total: null, unit: 'items', canStopWaiting: true, elapsedSecs: 1, cancellable: true, notCancellableReason: null });
      return Promise.resolve(true);
    }
    job.end(false);
    return Promise.resolve(true);
  }

  jobStopWaiting(jobId: number): Promise<boolean> {
    const job = this.held.get(jobId);
    if (!job?.canStop()) return Promise.resolve(false);
    job.end(true);
    return Promise.resolve(true);
  }

  private emitJob(event: JobEvent): void {
    for (const l of [...this.jobListeners]) l(event);
  }

  private async asJob<T>(method: keyof PitwallBackend, run: () => Promise<T>): Promise<T> {
    const jobId = ++this.jobSeq;
    this.emitJob({ kind: 'started', jobId, title: JOB_TITLES[method] ?? 'Working' });
    this.emitJob({ kind: 'progress', jobId, phase: 'Working on it', done: 1, total: null, unit: 'items', canStopWaiting: false, elapsedSecs: 1, cancellable: true, notCancellableReason: null });
    try {
      if (this.holdJobs) {
        let stopped = false;
        await new Promise<void>((resolve) => {
          this.held.set(jobId, {
            end: (stoppedWaiting) => {
              stopped = stoppedWaiting;
              resolve();
            },
            canStop: () => this.refuseCancel,
          });
        });
        this.held.delete(jobId);
        this.emitJob({ kind: 'cancelled', jobId, stoppedWaiting: stopped });
        const error: CommandError = { code: 'cancelled', message: stopped ? 'Stopped waiting. The server may still be working on it.' : 'Cancelled.' };
        throw error;
      }
      const value = await run();
      this.emitJob({ kind: 'done', jobId, result: value ?? null });
      return value;
    } catch (e) {
      if ((e as CommandError).code !== 'cancelled') this.emitJob({ kind: 'failed', jobId, error: e as CommandError });
      throw e;
    }
  }

  private reply<T>(method: keyof PitwallBackend, value: () => T): Promise<T> {
    const run = () => this.replyNow(method, value);
    return method in JOB_TITLES ? this.asJob(method, run) : run();
  }

  private async replyNow<T>(method: keyof PitwallBackend, value: () => T): Promise<T> {
    if (this.latency) await new Promise((r) => setTimeout(r, this.latency));
    if (this.failing.has(method) || (this.serverDown && !APP_METHODS.has(method))) {
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
      engineSetup: clone(this.engineSetup_),
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

  usageSeries(range: UsageRange, appId: string | null): Promise<UsageSeries> {
    return this.reply('usageSeries', () => demoUsageSeries(range, this.now(), appId, this.apps_.map((a) => a.id)));
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

  // ---- The tray popup's widgets (the engine's recorded output; see mock/mock-popover.ts) ---------------------------------------

  /** The scenario in force: the chosen one, as `paused` once serving is paused in the demo. */
  private scenario(): PopoverScenario {
    return this.popoverScenario === 'demo' && this.server.serving === 'paused' ? 'paused' : this.popoverScenario;
  }

  private pushPopover(): void {
    if (!this.popoverListeners.size) return;
    const push: PopoverPush = { view: this.popover_.view(), values: this.popover_.values(this.scenario(), this.now()), header: this.popover_.header(this.scenario()) };
    for (const l of [...this.popoverListeners]) l(clone(push));
  }

  popoverView(): Promise<PopoverView> {
    return this.reply('popoverView', () => this.popover_.view());
  }

  popoverValues(): Promise<PopoverValues> {
    return this.reply('popoverValues', () => this.popover_.values(this.scenario(), this.now()));
  }

  popoverHeader(): Promise<PopoverHeader> {
    return this.reply('popoverHeader', () => this.popover_.header(this.scenario()));
  }

  popoverShape(): Promise<LoadingRows[]> {
    return this.reply('popoverShape', () => LOADING_SHAPE);
  }

  editPopover(edit: PopoverEdit): Promise<PopoverView> {
    return this.reply('editPopover', () => {
      const view = this.popover_.edit(edit);
      queueMicrotask(() => this.pushPopover());
      return view;
    });
  }

  onPopover(handler: (push: PopoverPush) => void): Unsubscribe {
    this.popoverListeners.add(handler);
    return () => this.popoverListeners.delete(handler);
  }

  setServing(paused: boolean): Promise<void> {
    return this.reply('setServing', () => {
      this.server.serving = paused ? 'paused' : 'serving';
      this.emit({ kind: 'server', server: clone(this.server) });
      queueMicrotask(() => this.pushPopover());
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

  cacheInfo(): Promise<CacheInfo> {
    return this.reply('cacheInfo', () => {
      const now = this.now();
      const c = this.settings_.cache;
      const kind = (storage: CacheInfo['responses']['storage'], entries: number, bytesPerEntry: number, maxMib: number, ttl: number, sliding: boolean, hits: number, ageDays: number) => ({
        storage,
        entries,
        bytes: entries * bytesPerEntry,
        maxBytes: maxMib * 1024 * 1024,
        hits: entries ? hits : 0,
        oldestAt: entries ? now - ageDays * 86_400_000 : null,
        expiresAfterSecs: ttl === 0 ? null : ttl,
        sliding,
        diskError: null,
      });
      return {
        responses: kind(c.responsesStorage, this.cache_.responses, 2_900, c.responsesMaxMib, c.responsesTtlSecs, false, 212, 5.2),
        embeddings: kind(c.embeddingsStorage, this.cache_.embeddings, 3_300, c.embeddingsMaxMib, c.embeddingsTtlSecs, true, 2_310, 41),
        keepWarmFiles: 2,
        keepWarmBytes: 18 * 1024 * 1024,
        nextSweepAt: now + 3 * 3_600_000,
      };
    });
  }

  clearCache(scope: CacheClearScope): Promise<number> {
    return this.reply('clearCache', () => {
      const take = (key: 'responses' | 'embeddings', n: number) => {
        const removed = Math.min(this.cache_[key], n);
        this.cache_[key] -= removed;
        return removed;
      };
      switch (scope.kind) {
        case 'all':
          return take('responses', Infinity) + take('embeddings', Infinity);
        case 'responses':
          return take('responses', Infinity);
        case 'embeddings':
          return take('embeddings', Infinity);
        default:
          return take('responses', 40);
      }
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
      // Like the engine: a notice that offered this file is done once its download starts being tracked.
      this.updateNotices_ = this.updateNotices_.filter((n) => !(n.repo === repo && n.file === file));
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
        this.loaded.push({ modelId: id, name: m.name, engine: 'llama.cpp', build: 'cuda-13 · b11669', state: 'ready', ramBytes: m.sizeBytes * 1.1 - vram, vramBytes: vram, contextTokens: 8192, slots: m.slots, pinned: m.pinned, activeRequests: 0, loadedAt: now, lastUsedAt: now, idleReleaseAt: m.pinned ? null : now + (m.idleSecs ?? this.settings_.memory.idleUnloadSecs) * 1000, coldStartMs: 2100, capabilities: m.capabilities });
        m.loaded = true;
        m.loadedSlots = m.slots;
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

  setModelSlots(id: string, slots: number | null): Promise<SlotsChange> {
    return this.reply('setModelSlots', () => {
      if (slots !== null && (!Number.isInteger(slots) || slots < 1 || slots > 16)) this.fail('badRequest', 'Parallel slots must be between 1 and 16, not ' + slots + '.');
      const lib = this.library.find((x) => x.id === id) ?? this.fail('notFound', `No model ${id}.`);
      lib.slots = slots ?? DEFAULT_SLOTS;
      lib.slotsCustom = slots !== null;
      lib.fit = this.fitAt(lib);
      return { slots: lib.slots, loadedSlots: lib.loadedSlots, appliesOnNextLoad: lib.loadedSlots !== null && lib.loadedSlots !== lib.slots };
    });
  }

  modelFit(id: string, context: number | null, slots: number | null): Promise<SlotFit> {
    return this.reply('modelFit', () => {
      const lib = this.library.find((x) => x.id === id) ?? this.fail('notFound', `No model ${id}.`);
      if (context !== null && (context < 512 || context > 1_048_576)) this.fail('badRequest', 'The context must be between 512 and 1048576 tokens.');
      if (slots !== null && (!Number.isInteger(slots) || slots < 1 || slots > 16)) this.fail('badRequest', 'Parallel slots must be between 1 and 16, not ' + slots + '.');
      return demoSlotFit(lib, context ?? DEFAULT_CONTEXT, slots ?? lib.slots);
    });
  }

  /** The library fit at the model's slots (the table shows one verdict per row). */
  private fitAt(m: LocalModel): Fit {
    const f = demoSlotFit(m, DEFAULT_CONTEXT, m.slots);
    return { ...m.fit, verdict: f.verdict, message: f.verdict === 'wontFit' ? `${m.slots} slots do not fit; choose fewer or a shorter context.` : m.fit.message };
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

  removeLeftover(id: string): Promise<void> {
    return this.reply('removeLeftover', () => {
      const e = this.engines_[0]!;
      if (!e.leftovers.some((l) => l.id === id)) this.fail('notFound', `"${id}" is not something Pitwall left on disk.`);
      e.leftovers = e.leftovers.filter((l) => l.id !== id);
    });
  }

  installBuild(buildId: string): Promise<Download> {
    return this.reply('installBuild', () => {
      const engine = this.engineOfBuild(buildId) ?? this.fail('notFound', `No build ${buildId} in the catalog.`);
      const cat = engine.catalog.find((c) => c.id === buildId)!;
      const d: Download = { id: `eng-${this.nextId++}-${cat.id}`, kind: 'engine', name: buildDownloadName(engine.name, cat), host: 'github.com', bytesDone: 0, bytesTotal: cat.downloadBytes, speedBps: 0, state: 'queued', error: null, startedAt: this.now() };
      this.downloads_.unshift(d);
      this.emit({ kind: 'download', download: clone(d) });
      return d;
    });
  }

  retryEngineSetup(): Promise<EngineSetup> {
    return this.reply('retryEngineSetup', () => {
      this.engineSetup_ = { state: 'idle', error: null, download: null };
      this.emit({ kind: 'engineSetup', setup: clone(this.engineSetup_) });
      return clone(this.engineSetup_);
    });
  }

  /** Test and demo hook: shows the automatic llama.cpp install in the given state. */
  setEngineSetup(setup: EngineSetup): void {
    this.engineSetup_ = setup;
    this.emit({ kind: 'engineSetup', setup: clone(setup) });
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
      const before = e.builds.find((b) => b.active)?.id;
      e.buildChoice = choice;
      this.refreshActive(before);
    });
  }

  rollbackBuild(): Promise<PinChange> {
    return this.reply('rollbackBuild', () => {
      const e = this.engines_[0]!;
      const before = e.builds.find((b) => b.active)?.id;
      const { target, blocked } = this.rollbackTarget();
      if (!target) this.fail('conflict', blocked ?? 'There is no earlier engine build to go back to.');
      this.pins_.global = target.buildId;
      this.refreshActive(before);
      const later = before === target.buildId ? [] : this.loaded.map((l) => l.modelId);
      return this.pinChange(`Rolled back to ${target.buildId} ${target.tag} and pinned it for all models; unpin it to follow the automatic choice again.`, later);
    });
  }

  enginePins(): Promise<EnginePinState> {
    return this.reply('enginePins', () => this.pinState());
  }

  setGlobalPin(buildId: string | null): Promise<PinChange> {
    return this.reply('setGlobalPin', () => {
      const e = this.engines_[0]!;
      if (buildId !== null) this.checkPinnable(buildId, 'pinned for all models');
      const before = e.builds.find((b) => b.active)?.id;
      this.pins_.global = buildId;
      this.refreshActive(before);
      const now = e.builds.find((b) => b.active)?.id;
      return this.pinChange(buildId ? `${buildId} is pinned for all models.` : 'All models follow the automatic engine choice again.', before === now ? [] : this.loaded.map((l) => l.modelId));
    });
  }

  setModelPin(modelId: string, buildId: string | null): Promise<PinChange> {
    return this.reply('setModelPin', () => {
      if (!this.library.some((x) => x.id === modelId)) this.fail('notFound', `No model ${modelId}.`);
      if (buildId !== null) this.checkPinnable(buildId, `pinned for ${modelId}`);
      if (buildId === null) delete this.pins_.models[modelId];
      else this.pins_.models[modelId] = buildId;
      const running = this.loaded.some((l) => l.modelId === modelId) && buildId !== null && buildId !== this.engines_[0]!.builds.find((b) => b.active)?.id;
      return this.pinChange(buildId ? `${modelId} is pinned to ${buildId}.` : `${modelId} follows the global choice again.`, running ? [modelId] : []);
    });
  }

  buildCapabilities(): Promise<BuildCapabilities[]> {
    return this.reply('buildCapabilities', () => this.engines_[0]!.builds.map((b) => ({ buildId: b.id, tag: b.tag, capabilities: demoBuildCapabilities(b.tag) })));
  }

  /** A pin only goes to a build that is installed and passed its self-test (the engine's rule). */
  private checkPinnable(buildId: string, scope: string): void {
    const b = this.engines_[0]!.builds.find((x) => x.id === buildId);
    if (!b) this.fail('notFound', `The engine build ${buildId} is ${scope} but is not installed. Install it on the llama.cpp page, or remove the pin.`);
    if (b.selfTest !== 'passed') this.fail('conflict', `The engine build ${buildId} is ${scope} but failed its self-test. Reinstall it on the llama.cpp page, or remove the pin.`);
  }

  /** Recomputes which build serves models in general, remembers the one it replaced, and where roll back goes. */
  private refreshActive(before: string | undefined): void {
    const e = this.engines_[0]!;
    const pick = this.pins_.global
      ? e.builds.find((b) => b.id === this.pins_.global)
      : e.buildChoice === 'auto'
        ? (e.builds.find((b) => b.selfTest === 'passed' && b.id.startsWith('cuda')) ?? e.builds[0])
        : e.builds.find((b) => b.id === e.buildChoice);
    for (const b of e.builds) b.active = b === pick;
    if (before && pick && before !== pick.id) this.pins_.history = [before, ...this.pins_.history.filter((h) => h !== before)].slice(0, 8);
    e.rollbackTo = this.rollbackTarget().target?.buildId ?? null;
  }

  private rollbackTarget(): { target: RollbackTarget | null; blocked: string | null } {
    const e = this.engines_[0]!;
    const current = e.builds.find((b) => b.active)?.id;
    const failed: string[] = [];
    let gone = false;
    for (const id of this.pins_.history.filter((h) => h !== current)) {
      const b = e.builds.find((x) => x.id === id);
      if (b?.selfTest === 'passed') return { target: { buildId: b.id, tag: b.tag }, blocked: null };
      if (b) failed.push(id);
      else gone = true;
    }
    if (failed.length) return { target: null, blocked: `The earlier engine build(s) ${failed.join(', ')} failed their self-test, so Pitwall will not go back to them. Reinstall one on the llama.cpp page.` };
    return { target: null, blocked: gone ? 'The earlier engine builds are no longer installed. Install one on the llama.cpp page.' : 'There is no earlier engine build to go back to.' };
  }

  private pinState(): EnginePinState {
    const e = this.engines_[0]!;
    const tag = (id: string) => e.builds.find((b) => b.id === id)?.tag ?? null;
    const { target, blocked } = this.rollbackTarget();
    return {
      global: this.pins_.global,
      models: Object.entries(this.pins_.models).map(([modelId, buildId]) => ({ modelId, buildId, tag: tag(buildId) })),
      active: e.builds.find((b) => b.active)?.id ?? null,
      activeSource: this.pins_.global ? 'pinned for all models' : 'chosen automatically',
      rollbackTo: target,
      rollbackBlocked: blocked,
      problems: [],
    };
  }

  private pinChange(message: string, later: string[]): PinChange {
    const note = later.length ? ` Loaded models keep their current engine until they are loaded again: ${later.join(', ')}.` : '';
    this.engines_[0]!.rollbackTo = this.rollbackTarget().target?.buildId ?? null;
    return { state: this.pinState(), appliesOnNextLoad: later, message: message + note };
  }

  // ---- Model presets and aliases -----------------------------------------------------------------------------

  /** What a target resolves to, a simplified mirror of the engine's rules (the real engine is the reference). */
  private presetStatus(p: ModelPreset): PresetStatus {
    if (!p.target.startsWith('auto')) {
      const m = this.library.find((x) => x.id === p.target);
      return m
        ? { state: 'ready', modelId: m.id, chosenBy: 'explicit', reason: `the preset ${p.alias} names this model`, loaded: m.loaded }
        : { state: 'stale', message: `the preset "${p.alias}" points to the model "${p.target}", which is no longer installed; edit the preset in Models > Presets` };
    }
    const words = p.target.slice('auto'.length + 1).split(/[,+ ]+/).filter(Boolean);
    const caps = words.filter((w) => CAPABILITY_WORDS.includes(w));
    const prefer = words.find((w) => PREFER_WORDS.includes(w)) ?? 'balanced';
    const fits = this.library.filter((m) => (caps.length ? caps : ['chat']).every((c) => m.capabilities.includes(c as never)));
    if (!fits.length) return { state: 'noMatch', message: `the preset "${p.alias}" matched no model: no installed model offers ${(caps.length ? caps : ['chat']).join(', ')}; download one in Pitwall > Models` };
    const size = (m: LocalModel) => m.paramsB ?? 0;
    const pick = prefer === 'quality' ? fits.reduce((a, b) => (size(b) > size(a) ? b : a)) : prefer === 'balanced' ? (fits.find((m) => m.loaded) ?? fits[0]!) : fits.reduce((a, b) => (size(b) < size(a) ? b : a));
    return { state: 'ready', modelId: pick.id, chosenBy: 'capabilities', reason: `the preset ${p.alias}: chosen for ${(caps.length ? caps : ['chat']).join(', ')}`, loaded: pick.loaded };
  }

  /** The first thing wrong with a draft, in words, or null. A simplified mirror of the engine's validation. */
  private presetProblem(d: PresetDraft, editing: string | null): string | null {
    const alias = d.alias.trim().toLowerCase();
    if (!/^[a-z0-9][a-z0-9._-]{0,47}$/.test(alias) || alias.includes('..')) return `"${d.alias}" is not a valid preset name: use up to 48 letters, digits and the characters "-_.", starting with a letter or digit`;
    if (alias === 'auto' || alias === 'default') return `"${alias}" is reserved (it already means Pitwall's own choice of model); choose another name`;
    const target = d.target.trim().toLowerCase();
    if (target === 'auto' || target.startsWith('auto:')) {
      const bad = target.slice(5).split(/[,+ ]+/).filter(Boolean).find((w) => !CAPABILITY_WORDS.includes(w) && !PREFER_WORDS.includes(w) && !/^\d+k?$/.test(w));
      if (bad) return `"${bad}" is not a capability (chat, vision, tools, json, embeddings, rerank), a context like 8k, or fast/quality/small`;
    } else if (!this.library.some((m) => m.id === target)) {
      return `the model "${target}" is not installed; pick an installed model or a capability selection such as auto:chat`;
    }
    if (this.library.some((m) => m.id === alias)) return `"${alias}" is the id of an installed model; choose another name so requests for the model are not redirected`;
    if (d.context !== null && (d.context < 512 || d.context > 1_048_576)) return 'context: use between 512 and 1048576 tokens';
    if (d.temperature !== null && (d.temperature < 0 || d.temperature > 2)) return 'temperature: use a value between 0 and 2';
    if (d.topP !== null && (d.topP < 0 || d.topP > 1)) return 'top-p: use a value between 0 and 1';
    if (d.maxTokens !== null && (d.maxTokens < 1 || d.maxTokens > 1_048_576)) return 'max tokens: use between 1 and 1048576';
    if (d.systemPrompt !== null && d.systemPrompt.length > 8192) return 'system prompt: use at most 8 KB of text';
    if (alias !== editing && this.presets_.some((p) => p.alias === alias)) return `a preset named "${alias}" already exists`;
    return null;
  }

  private presetFrom(d: PresetDraft): ModelPreset {
    return { ...d, alias: d.alias.trim().toLowerCase(), target: d.target.trim().toLowerCase(), systemPrompt: d.systemPrompt?.trim() ? d.systemPrompt : null };
  }

  presets(): Promise<PresetEntry[]> {
    return this.reply('presets', () => [...this.presets_].sort((a, b) => a.alias.localeCompare(b.alias)).map((preset) => ({ preset, status: this.presetStatus(preset) })));
  }

  createPreset(draft: PresetDraft): Promise<ModelPreset> {
    return this.reply('createPreset', () => {
      const problem = this.presetProblem(draft, null);
      if (problem) this.fail(problem.includes('already exists') || problem.includes('installed model;') ? 'conflict' : 'badRequest', problem);
      if (this.presets_.length >= 64) this.fail('conflict', 'there are already 64 presets; delete one first');
      const preset = this.presetFrom(draft);
      this.presets_.push(preset);
      return preset;
    });
  }

  updatePreset(alias: string, draft: PresetDraft): Promise<ModelPreset> {
    return this.reply('updatePreset', () => {
      const i = this.presets_.findIndex((p) => p.alias === alias);
      if (i < 0) this.fail('notFound', `there is no preset named "${alias}"`);
      const problem = this.presetProblem(draft, alias);
      if (problem) this.fail(problem.includes('already exists') ? 'conflict' : 'badRequest', problem);
      const preset = this.presetFrom(draft);
      this.presets_[i] = preset;
      return preset;
    });
  }

  deletePreset(alias: string): Promise<void> {
    return this.reply('deletePreset', () => {
      const i = this.presets_.findIndex((p) => p.alias === alias);
      if (i < 0) this.fail('notFound', `there is no preset named "${alias}"`);
      this.presets_.splice(i, 1);
    });
  }

  resolvePreset(draft: PresetDraft, editing: string | null): Promise<PresetPreview> {
    return this.reply('resolvePreset', () => {
      const problem = this.presetProblem(draft, editing);
      // Like the engine: the target's status is shown whenever the target itself parses, even if the name is wrong.
      const targetParses = !problem?.includes('is not a capability');
      return { problem, status: targetParses ? this.presetStatus(this.presetFrom({ ...draft, alias: 'preview' })) : null };
    });
  }

  // ---- Model update notices ------------------------------------------------------------------------------------

  private updateStatus(): UpdateCheckStatus {
    const week = this.updateCheck_.schedule === 'daily' ? 86_400_000 : 7 * 86_400_000;
    return {
      settings: { ...this.updateCheck_ },
      lastCheckedAt: this.updateChecked_?.at ?? null,
      lastAttemptAt: this.updateChecked_?.at ?? null,
      lastMessage: this.updateChecked_?.message ?? null,
      nextCheckAt: !this.updateCheck_.enabled ? null : (this.updateChecked_?.at ?? this.now()) + week,
      running: false,
      notices: this.visibleUpdates().length,
      host: 'huggingface.co',
      maxRepos: 40,
    };
  }

  private visibleUpdates(): UpdateNotice[] {
    return this.updateNotices_.filter((n) => !this.updateHidden_.has(n.id) && this.library.some((m) => m.id === n.modelId));
  }

  updateCheckStatus(): Promise<UpdateCheckStatus> {
    return this.reply('updateCheckStatus', () => this.updateStatus());
  }

  updateNotices(): Promise<UpdateNotice[]> {
    return this.reply('updateNotices', () => clone(this.visibleUpdates()));
  }

  checkUpdatesNow(): Promise<UpdateCheckResult> {
    return this.reply('checkUpdatesNow', () => {
      const now = this.now();
      if (this.updateChecked_ && now - this.updateChecked_.at < 60_000) {
        const wait = Math.ceil((60_000 - (now - this.updateChecked_.at)) / 1000);
        return { outcome: 'rateLimited', message: `A check ran moments ago. Try again in ${wait} seconds.`, reposChecked: 0, notices: this.visibleUpdates().length, retryInSecs: wait };
      }
      if (this.updateNotices_.length === 0) this.updateNotices_ = demoUpdateNotices(now);
      const repos = new Set(this.library.map((m) => m.repo)).size;
      const message = `Asked Hugging Face about ${repos} repositories.`;
      this.updateChecked_ = { at: now, message };
      return { outcome: 'checked', message, reposChecked: repos, notices: this.visibleUpdates().length, retryInSecs: null };
    });
  }

  setUpdateCheck(enabled: boolean, schedule: UpdateSchedule): Promise<UpdateCheckStatus> {
    return this.reply('setUpdateCheck', () => {
      this.updateCheck_ = { enabled, schedule };
      return this.updateStatus();
    });
  }

  dismissUpdate(id: string, snoozeDays: number | null): Promise<UpdateNotice[]> {
    return this.reply('dismissUpdate', () => {
      if (!this.updateNotices_.some((n) => n.id === id)) this.fail('notFound', `there is no update notice "${id}" (it may already have been dealt with)`);
      if (snoozeDays !== null && (snoozeDays < 1 || snoozeDays > 90)) this.fail('badRequest', `a snooze lasts between 1 and 90 days (it was ${snoozeDays})`);
      this.updateHidden_.add(id);
      return clone(this.visibleUpdates());
    });
  }

  // ---- Apps --------------------------------------------------------------------------------------------------

  apps(): Promise<AppInfo[]> {
    return this.reply('apps', () => this.apps_);
  }

  appDetail(appId: string): Promise<AppDetail> {
    return this.reply('appDetail', () => {
      const app = this.apps_.find((a) => a.id === appId) ?? this.fail('notFound', `No app ${appId}.`);
      return demoAppDetail(app, this.requests_, this.now());
    });
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
        permissions: { models: null, maxPriority: 'background', tools: false, extendedApis: false, cacheResponses: false, limits: { requestsPerMin: 60, tokensPerDay: null, maxConcurrent: 1, requestsPerDay: null, tokensPerMonth: null, maxMemoryPercent: null } },
        today: { requests: 0, promptTokens: 0, completionTokens: 0, busyMs: 0, errors: 0, peakMemoryBytes: 0, cacheHits: 0, cacheMisses: 0, prefixTokens: 0, savedTokens: 0 },
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
      return clone(app);
    });
  }

  approvePairing(id: string, permissions: AppPermissions): Promise<void> {
    return this.reply('approvePairing', () => {
      const p = this.pairings_.find((x) => x.id === id) ?? this.fail('notFound', 'That pairing request has expired.');
      this.pairings_ = this.pairings_.filter((x) => x !== p);
      if (!this.apps_.some((a) => a.id === p.appId)) {
        this.apps_.push({ id: p.appId, name: p.appName, suite: false, connected: true, lastSeenAt: this.now(), keys: [{ id: `${p.appId}-k1`, prefix: 'pw_8c1e…', createdAt: this.now(), lastUsedAt: null }], permissions: clone(permissions), today: { requests: 0, promptTokens: 0, completionTokens: 0, busyMs: 0, errors: 0, peakMemoryBytes: 0, cacheHits: 0, cacheMisses: 0, prefixTokens: 0, savedTokens: 0 } });
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
    const words = demoReplyWords(request.modelId);
    const limit = wordsThatFit(words, request.maxTokens);
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
      const cold = !this.loaded.some((m) => m.modelId === request.modelId);
      if (cold) onEvent({ kind: 'phase', phase: { kind: 'loading', elapsedMs: 0 } });
      onEvent({ kind: 'phase', phase: { kind: 'generating' } });
      const step = () => {
        if (stopped) return;
        if (i < limit) {
          onEvent({ kind: 'delta', text: words[i++]! });
          timers.push(setTimeout(step, this.streamDelay));
        } else {
          const fast = request.modelId.includes('qwen3.5-4b');
          onEvent({ kind: 'done', finish: limit < words.length ? 'length' : 'stop', promptTokens: 18 + request.messages.reduce((s, m) => s + Math.ceil(m.content.length / 4), 0), completionTokens: Math.round(limit * 1.3), ttftMs: fast ? 96 : 214, genTps: fast ? 118.4 : 61.8, coldStartMs: this.loaded.some((m) => m.modelId === request.modelId) ? null : 2100 });
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
      this.server.port = settings.server.port;
      this.server.lan = settings.network.lanEnabled
        ? { enabled: true, url: `https://192.168.1.42:${settings.network.lanPort}`, certFingerprint: '4F:9A:21:C0:7E:55:B3:18:9D:02:6A:F1:3C:88:E4:57:0B:A9:D6:12:7F:C3:45:8E:91:2D:B0:66:EA:73:19:5C' }
        : { enabled: false, url: null, certFingerprint: null };
      this.emit({ kind: 'server', server: clone(this.server) });
      return this.settings_;
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

  /** How often Repair was pressed (tests). */
  repairs = 0;

  repairService(): Promise<ServiceInstall> {
    return this.reply('repairService', () => {
      this.repairs += 1;
      this.serverDown = false;
      this.service = {
        ...this.service,
        running: true,
        startFailed: false,
        notInLoginItems: false,
        message: null,
        repair: { state: 'succeeded', message: 'Service is running again' },
      };
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

  /** How often "Open Login Items" was asked for (tests). */
  loginItemsOpened = 0;

  openLoginItems(): Promise<void> {
    return this.reply('openLoginItems', () => {
      this.loginItemsOpened += 1;
    });
  }

  /** Quits asked of the mock app: whether serving was stopped and the choice remembered. */
  quits: { stopServing: boolean; remember: boolean }[] = [];
  private quitAskPending = false;
  private quitAskHandlers = new Set<() => void>();

  quitApp(stopServing: boolean, remember: boolean): Promise<void> {
    return this.reply('quitApp', () => {
      this.quits.push({ stopServing, remember });
      if (remember) this.settings_.general.onQuit = stopServing ? 'stopServing' : 'keepServing';
      if (stopServing) {
        this.loaded = [];
        this.server.serving = 'stopped';
        this.emit({ kind: 'server', server: clone(this.server) });
      }
    });
  }

  chatEnabled(): Promise<boolean> {
    return this.reply('chatEnabled', () => this.chat_);
  }

  /** Dyno as the mock knows it: a key that starts `pmk_` connects, `pmk_wrong` is refused, and Dyno is "open". */
  private dyno_: ConnectedApp = { id: 'dyno', title: 'Dyno', keySaved: false, connected: false, problem: null, keyRefused: false, reconnecting: false, needs: null, tools: [], remembered: 0 };
  private readonly appsHandlers = new Set<(apps: ConnectedApp[]) => void>();

  onConnectedApps(handler: (apps: ConnectedApp[]) => void): Unsubscribe {
    this.appsHandlers.add(handler);
    return () => this.appsHandlers.delete(handler);
  }

  /** As the app does after every change: every window hears the apps. */
  private appsChanged(): ConnectedApp {
    for (const h of this.appsHandlers) h([clone(this.dyno_)]);
    return clone(this.dyno_);
  }

  connectedApps(): Promise<ConnectedApp[]> {
    return this.reply('connectedApps', () => [clone(this.dyno_)]);
  }

  setAppKey(appId: string, key: string): Promise<ConnectedApp> {
    return this.reply('setAppKey', () => {
      const k = key.trim();
      if (appId !== 'dyno') throw { code: 'bad_request', message: `Pitwall does not know an app called ${appId}.` };
      if (!/^pmk_\S+$/.test(k)) throw { code: 'bad_request', message: 'That is not an AI access key. Copy the key Dyno shows in Settings > AI access (it starts with pmk_).' };
      this.dyno_ = { ...this.dyno_, keySaved: true };
      return this.connectDyno(k !== 'pmk_wrong');
    });
  }

  connectApp(appId: string): Promise<ConnectedApp> {
    return this.reply('connectApp', () => {
      if (appId !== 'dyno') throw { code: 'bad_request', message: `Pitwall does not know an app called ${appId}.` };
      if (!this.dyno_.keySaved) {
        this.dyno_ = { ...this.dyno_, problem: "No key is saved for Dyno. Issue one in Dyno's Settings > AI access and paste it here." };
        return this.appsChanged();
      }
      return this.connectDyno(!this.dyno_.keyRefused);
    });
  }

  forgetApp(appId: string): Promise<ConnectedApp> {
    return this.reply('forgetApp', () => {
      if (appId !== 'dyno') throw { code: 'bad_request', message: `Pitwall does not know an app called ${appId}.` };
      this.dyno_ = { id: 'dyno', title: 'Dyno', keySaved: false, connected: false, problem: null, keyRefused: false, reconnecting: false, needs: null, tools: [], remembered: 0 };
      return this.appsChanged();
    });
  }

  /** Dyno "opens" and, as the app's timer does, is connected right after when a key is saved. */
  openConnectedApp(appId: string): Promise<OpenedApp> {
    return this.reply('openConnectedApp', () => {
      if (appId !== 'dyno') return { kind: 'problem', message: `Pitwall does not know an app called ${appId}.` } as OpenedApp;
      if (this.dynoMissing) {
        // As Rust does: the row's problem is published (it can arrive before this answer), then the same words are answered.
        const message = "Dyno isn't installed on this computer. Install Dyno, then choose Open Dyno.";
        this.dyno_ = { ...this.dyno_, connected: false, reconnecting: false, needs: 'install', tools: [], problem: message };
        this.appsChanged();
        return { kind: 'problem', message } as OpenedApp;
      }
      if (this.dyno_.keySaved && !this.dyno_.keyRefused) queueMicrotask(() => this.connectDyno(true));
      return { kind: 'started' } as OpenedApp;
    });
  }

  /** The chats as the mock knows them: the default for new chats, which chats are not saved, and a few saved ones. */
  private chatStorage_: ChatStorage = { saveNew: true, chats: 3, bytes: 48_000, attachmentBytes: 0, unsaved: 0 };
  /** Chats the user marked "Don't save" (tests read it). */
  readonly unsavedChats = new Set<string>();

  chatStorage(): Promise<ChatStorage> {
    return this.reply('chatStorage', () => clone({ ...this.chatStorage_, unsaved: this.unsavedChats.size }));
  }

  setChatSaveNew(save: boolean): Promise<ChatStorage> {
    return this.reply('setChatSaveNew', () => {
      this.chatStorage_ = { ...this.chatStorage_, saveNew: save };
      return clone({ ...this.chatStorage_, unsaved: this.unsavedChats.size });
    });
  }

  chatSaved(conversationId: string): Promise<boolean> {
    return this.reply('chatSaved', () => !this.unsavedChats.has(conversationId));
  }

  setChatSaved(conversationId: string, save: boolean): Promise<boolean> {
    return this.reply('setChatSaved', () => {
      if (save) this.unsavedChats.delete(conversationId);
      else this.unsavedChats.add(conversationId);
      return save;
    });
  }

  deleteAllChats(): Promise<ChatStorage> {
    return this.reply('deleteAllChats', () => {
      this.unsavedChats.clear();
      this.chatStorage_ = { ...this.chatStorage_, chats: 0, bytes: 0, attachmentBytes: 0 };
      return clone({ ...this.chatStorage_, unsaved: 0 });
    });
  }

  /** Demo and test hook: Dyno is not installed, so Open Dyno answers that. */
  dynoUninstalled(): void {
    this.dynoMissing = true;
  }
  private dynoMissing = false;

  /** Demo and test hook: a retry met a Dyno too old for Pitwall; nothing is retried until the user updates it. */
  dynoTooOld(): void {
    this.dynoMissing = false;
    this.dyno_ = { ...this.dyno_, connected: false, reconnecting: false, needs: 'update', tools: [], problem: 'Dyno is too old for AI access from Pitwall. Update Dyno, then choose Refresh tools.' };
    this.appsChanged();
  }

  /** Demo and test hook: Dyno quit (its connection ended); the app retries until it is back. */
  dynoQuit(): void {
    this.dyno_ = { ...this.dyno_, connected: false, reconnecting: true, tools: [], problem: 'Dyno closed the connection (the app quit). Trying again when Dyno is back.' };
    this.appsChanged();
  }

  revokeToolApproval(appId: string, tool: string): Promise<ConnectedApp> {
    return this.reply('revokeToolApproval', () => {
      if (appId !== 'dyno') throw { code: 'bad_request', message: `Pitwall does not know an app called ${appId}.` };
      const tools = this.dyno_.tools.map((t) => (t.name === tool ? { ...t, remembered: false } : t));
      this.dyno_ = { ...this.dyno_, tools, remembered: tools.filter((t) => t.remembered).length };
      return this.appsChanged();
    });
  }

  private connectDyno(keyOk: boolean): ConnectedApp {
    this.dyno_ = keyOk
      ? {
          ...this.dyno_,
          connected: true,
          problem: null,
          keyRefused: false,
          reconnecting: false,
          needs: null,
          tools: [
            { name: 'alarms', title: 'Alarms', readOnly: true, remembered: false },
            { name: 'cpu_memory', title: 'CPU and memory', readOnly: true, remembered: false },
            { name: 'disks', title: 'Disks', readOnly: true, remembered: false },
            // One tool always allowed already (as after a chat), so Settings shows "Ask again".
            { name: 'system_summary', title: 'System summary', readOnly: true, remembered: true },
            { name: 'top_processes', title: 'Top processes', readOnly: true, remembered: false },
            { name: 'keep_awake', title: 'Keep this Mac awake', readOnly: false, remembered: false },
          ],
          remembered: 1,
        }
      : { ...this.dyno_, connected: false, keyRefused: true, tools: [], problem: 'Dyno refused the key. Issue a new key in Dyno\'s Settings > AI access and paste it here.' };
    return this.appsChanged();
  }

  private chatModel_: string | null = null;

  chatModels(): Promise<ChatModels> {
    return this.reply('chatModels', () => this.picker(this.chatModel_));
  }

  setChatModel(modelId: string): Promise<ChatModels> {
    return this.reply('setChatModel', () => this.picker(modelId));
  }

  /** The picker as Rust builds it (`chat_host::models`): tool models first, the others disabled with the reason. */
  private picker(wanted: string | null): ChatModels {
    const chat = this.library.filter((m) => m.capabilities.includes('chat'));
    const models = chat
      .map((m) => ({ id: m.id, name: m.name, loaded: m.loaded, disabled: m.capabilities.includes('tools') ? null : "Can't use tools" }))
      .sort((a, b) => Number(a.disabled !== null) - Number(b.disabled !== null) || Number(!a.loaded) - Number(!b.loaded) || a.name.localeCompare(b.name));
    // The starter catalog's tool models not downloaded yet (as Rust lists them).
    models.push({ id: 'qwen3.8-27b', name: 'Qwen3.8 27B', loaded: false, disabled: 'Not downloaded' });
    const usable = (id: string | null) => models.some((m) => m.id === id && m.disabled === null);
    const selected = usable(wanted) ? wanted : (models.find((m) => m.disabled === null)?.id ?? null);
    this.chatModel_ = selected;
    const notes: Record<string, string> = { 'Not downloaded': 'Starter models that can use tools: download one in Models, then choose it here.' };
    if (models.some((m) => m.disabled === "Can't use tools")) notes["Can't use tools"] = '"Can\'t use tools": the model\'s chat template has no tool calls, so it can\'t use a connected app\'s tools.';
    return { models, selected, notes };
  }

  takeQuitAsk(): Promise<boolean> {
    return this.reply('takeQuitAsk', () => {
      const pending = this.quitAskPending;
      this.quitAskPending = false;
      return pending;
    });
  }

  onQuitAsk(handler: () => void): Unsubscribe {
    this.quitAskHandlers.add(handler);
    return () => this.quitAskHandlers.delete(handler);
  }

  /**
   * Demo and test hook: the user chose Quit while the setting is "ask" (`windowOpen`: the window is there to answer). As
   * the app does, the request is left pending first and the event only tells the window to take it.
   */
  askQuit(windowOpen = true): void {
    this.quitAskPending = true;
    if (windowOpen) this.quitAskHandlers.forEach((h) => h());
  }

  /** Test hook: a stale `quit-ask` event with nothing pending (it must not bring the question up). */
  emitStaleQuitAsk(): void {
    this.quitAskHandlers.forEach((h) => h());
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
