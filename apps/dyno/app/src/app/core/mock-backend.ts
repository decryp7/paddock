import { osTerms } from '../shared/os-terms';
import type { SuJobPhase, SuJobSnapshot, SuJobState } from '@decryp7/paddock-ui';
import type { Measured } from './api.types';
import type { DynoSurface, MetricReading, Navigate, ReadingQuery, SurfaceEdit, SurfaceView, WidgetActionId, WidgetChoices } from './generated/widgets';
import { MockWidgets, mockMetricReading, mockWidgetChoices, mockWidgetOptionsFromUrl, type MockWidgetsOptions } from './mock/widgets';
import type { PopoverSurfaceValues } from './widget-types';
import { readValue } from './measured';
import type {
  Alarm,
  AlarmChange,
  AlarmFilter,
  AlarmStats,
  BundlePreview,
  HostName,
  RecorderSnapshot,
  RecorderSnapshotInfo,
  SavedDiagnostics,
  ShelveFor,
  ActivityEntry,
  AgentStatus,
  Alert,
  Anomaly,
  CommandError,
  Diagnosis,
  HealthReport,
  History,
  HistoryRange,
  LiveDetail,
  LiveUpdate,
  Locker,
  PluginInfo,
  ProcessDetails,
  ProcessRef,
  ProcessSample,
  CustomAlertRule,
  Settings,
  DiskChildren,
  DiskChildrenQuery,
  DiskRings,
  DiskRingsQuery,
  DiskScanStatus,
  DiskTarget,
  Volume,
  SnapshotDiff,
  SnapshotInfo,
  StartupEntry,
  SystemInventory,
  ThrottleOverview,
  LifespanOverview,
  BaselineOverview,
  BenchComparison,
  CounterHistory,
  CounterHit,
  CounterObject,
  CounterObjectList,
  CounterReading,
  NetworkQuery,
  NetworkSnapshot,
  BenchOverview,
  BenchProgress,
  BenchRun,
  StartBenchmark,
  SystemSample,
  Timestamp,
  WeeklyReport,
  ReportFormat,
  SavedReport,
  BackupReport,
  ChangeTimeline,
  ChangesQuery,
  FinderQuery,
  GpuReport,
  InventoryFormat,
  MaintenanceStatus,
  SavedInventory,
  IssuesOverview,
  IssuesQuery,
  JobOutput,
  JobSpec,
  SavedIssues,
  UnusualItem,
  UnusualOverview,
  HashSource,
  SearchSeed,
  StartupFilter,
  StartupOverview,
} from './api.types';
import type { DynoBackend, Unsubscribe } from './backend';
import {
  demoActivity,
  demoAlerts,
  demoAnomalies,
  demoPlugins,
  demoSettings,
  demoStartup,
  healthReasons,
} from './mock/data';
import { demoAlarms, demoBundlePreview, demoHistory, demoSnapshot, demoSnapshotInfos, demoStats, filterAlarms, isShelved } from './mock/alarms';
import { MockCounters, demoCounterSettings } from './mock/counters';
import { MockDiskMap } from './mock/disk-map';
import { mockHostName, mockNetwork } from './mock/network';
import { mockBoot, mockChanges, mockFinder, mockGpu, mockMaintenanceProblem, mockMaintenanceStatus, mockSleep, mockSoftware, mockUsage } from './mock/extras';
import { type MockBackupVariant, mockBackupProblem, mockBackupReport } from './mock/backup';
import { mockIssuesOverview, mockSources, mockUnusual, seedUnusual } from './mock/issues';
import { mockCompare, mockKindResult, mockRun } from './mock/bench';
import { mockHash, mockSearchSeed, mockSearchUrl, mockStartupOverview } from './mock/lookups';
import { mockHistory } from './mock/history';
import { mockBaselines, mockLifespan, mockThrottling } from './mock/hardware';
import { mockInventory } from './mock/inventory';
import { type MockOs, type OsProfile, coreKindsOf, osProfile } from './mock/os';
import { clamp, wobble } from './mock/noise';
import { type MockProcess, buildProcesses, groupSamples, round, sampleProcess } from './mock/processes';

const GB = 1024 ** 3;
const MB = 1024 ** 2;
const MIN = 60_000;
const DAY = 24 * 60 * MIN;

/** Which machine the mock pretends to be: the laptop has everything; the desktop lacks a battery and sensors. */
export type MockMachine = 'laptop' | 'desktop';

export type { MockOs };

export interface MockOptions {
  machine?: MockMachine;
  /** The operating system the mock pretends to run on (default macOS). */
  os?: MockOs;
  /** The CPU layout: `hybrid` (performance + efficiency cores; the default on macOS) or `uniform`. */
  cpu?: 'hybrid' | 'uniform';
  /** Simulated round-trip per request (skeletons show in the browser; 0 in unit tests). */
  latencyMs?: number;
  /** The clock (Playwright freezes it for stable screenshots). */
  now?: () => number;
  /** Requests that reject with an internal error (to see error states: `?fail=health,whySlow`). */
  fail?: ReadonlySet<string>;
  /** Which backup tools the mock finds (`?backup=none` finds none; default a mix). */
  backup?: MockBackupVariant;
  /** Where the mock pushes the status of the jobs it runs (the shell's in-memory jobs backend); jobs run without one too. */
  jobs?: MockJobSink;
  /** Keep the widget layouts in `localStorage` (the browser build; unit tests keep them in memory). */
  widgetStorage?: boolean;
  /** The popup's states and agent (`?popup=…&agent=stopped`). */
  widgets?: Pick<MockWidgetsOptions, 'popup' | 'agentStopped'>;
  /** How many steps each job the mock starts takes before it ends (default 1: it ends at once; `?jobsteps=6` keeps it running for screenshots). */
  jobSteps?: number;
}

/**
 * Reads the mock options from the page's query string: `?mock=` takes `windows`, `macos`, `linux` and/or `desktop`
 * (comma separated, e.g. `mock=linux,desktop`), `fail=health,whySlow` makes requests fail, `latency=0` removes the delay, `jobsteps=6` makes every job take that many steps.
 */
export function mockOptionsFromUrl(search: string): MockOptions {
  const q = new URLSearchParams(search);
  const fail = q.get('fail');
  const latency = q.get('latency');
  const jobSteps = q.get('jobsteps');
  const tokens = (q.get('mock') ?? '').split(',').map((t) => t.trim().toLowerCase());
  const os = (['windows', 'macos', 'linux'] as const).find((o) => tokens.includes(o));
  const cpu = tokens.includes('hybrid') ? 'hybrid' : tokens.includes('uniform') ? 'uniform' : undefined;
  return {
    machine: tokens.includes('desktop') ? 'desktop' : 'laptop',
    os: os ?? 'macos',
    ...(cpu ? { cpu } : {}),
    latencyMs: latency !== null && Number.isFinite(Number(latency)) ? Number(latency) : 150,
    fail: new Set(fail ? fail.split(',') : []),
    widgetStorage: true,
    widgets: mockWidgetOptionsFromUrl(search),
    ...(q.get('backup') === 'none' ? { backup: 'none' as const } : {}),
    ...(jobSteps !== null && Number.isInteger(Number(jobSteps)) && Number(jobSteps) >= 1 && Number(jobSteps) <= 100 ? { jobSteps: Number(jobSteps) } : {}),
  };
}

const copy = <T>(v: T): T => structuredClone(v);

/** `localStorage` when the browser build asked for it and it can be used. */
function browserStorage(wanted: boolean | undefined): Storage | null {
  try {
    return wanted && typeof localStorage !== 'undefined' ? localStorage : null;
  } catch {
    return null;
  }
}

/** Makes the window chrome follow the mock's OS in the browser build (the Tauri shell sets these before boot). */
export function applyMockPlatform(search: string, root: HTMLElement): void {
  if (root.dataset['platform']) return;
  // `chrome=macos|windows|linux` shows another OS's window chrome and shortcut glyphs than the mock machine's (screenshots).
  const chrome = new URLSearchParams(search).get('chrome');
  const os = chrome === 'macos' || chrome === 'windows' || chrome === 'linux' ? chrome : (mockOptionsFromUrl(search).os ?? 'macos');
  root.dataset['platform'] = os;
}

/**
 * The backend for browser development and UI tests: a developer's laptop (macOS by default) in the middle of a `cargo
 * build`. Live values are a deterministic function of the clock; the few requests that change anything (settings,
 * snapshots, benchmarks) change the mock's state so those flows can be exercised. It mimics the contract; it is never the reference for behaviour.
 */
/** A mock reading as a plain number: the mock reads everything. */
const num = (m: Measured<number>): number => readValue(m) ?? 0;

export class MockBackend implements DynoBackend {
  private readonly machine: MockMachine;
  private readonly latencyMs: number;
  private readonly now: () => number;
  private readonly fail: ReadonlySet<string>;
  private readonly start: number;
  private readonly profile: OsProfile;
  private readonly kinds: ('performance' | 'efficiency')[];

  private processes: MockProcess[];
  private activityLog: ActivityEntry[];
  private alertLog: Alert[];
  private settingsValue: Settings = demoSettings();
  private counters!: MockCounters;
  private startup: StartupEntry[] = demoStartup();
  private snapshotList: SnapshotInfo[];
  private readonly snapshotData = new Map<string, ProcessSample[]>();
  private nextId = 100;

  private readonly liveListeners = new Map<(u: LiveUpdate) => void, LiveDetail>();
  private readonly alertListeners = new Set<(a: Alert) => void>();
  private readonly activityListeners = new Set<(a: ActivityEntry) => void>();
  private readonly agentListeners = new Set<(s: AgentStatus) => void>();
  private readonly alarmListeners = new Set<(c: AlarmChange) => void>();
  /** The widget layouts and the popup's values (family ADR 0035), kept in the browser's storage so a reload keeps them. */
  readonly widgets: MockWidgets;
  private readonly disk: MockDiskMap;
  private unusualItems: UnusualItem[] = seedUnusual();
  private unusualForgotten = false;
  /** Issue exports the mock "saved", for tests. */
  readonly savedIssues: number[] = [];
  /** Inventory reports the mock "saved", for tests. */
  readonly savedInventories: InventoryFormat[] = [];
  /** Folder choices the mock was asked to show, for tests. */
  pickedFolders = 0;
  private readonly backupVariant: MockBackupVariant;
  private backupCheckedAt: number;
  private alarmList: Alarm[] | null = null;
  private snapshotInfos: RecorderSnapshotInfo[] | null = null;
  /** Bundles the mock "saved", for tests. */
  readonly savedBundles: SavedDiagnostics[] = [];
  private timer: ReturnType<typeof setInterval> | null = null;

  constructor(options: MockOptions = {}) {
    this.machine = options.machine ?? 'laptop';
    const base = osProfile(options.os ?? 'macos');
    this.profile =
      options.cpu === 'uniform' ? { ...base, hybrid: null } : options.cpu === 'hybrid' && !base.hybrid ? { ...base, hybrid: { efficiency: 8, performance: 8, efficiencyFirst: false } } : base;
    this.kinds = coreKindsOf(this.profile.hybrid);
    this.startup = demoStartup(this.profile.os);
    this.counters = new MockCounters(this.profile.os);
    this.settingsValue = { ...this.settingsValue, counters: demoCounterSettings(this.profile.os) };
    this.latencyMs = options.latencyMs ?? 0;
    this.jobSink = options.jobs;
    this.jobSteps = options.jobSteps ?? 1;
    this.jobStepMs = this.latencyMs === 0 ? 0 : 400;
    this.now = options.now ?? (() => Date.now());
    this.fail = options.fail ?? new Set();
    this.widgets = new MockWidgets({ storage: browserStorage(options.widgetStorage), now: this.now, ...(options.widgets ?? {}) });
    // A scan reports a few times: one timer per step (zero delay with no latency, so a test sees the first status before the rest).
    const latency = this.latencyMs;
    this.disk = new MockDiskMap(this.now, (fn, step) => setTimeout(fn, latency === 0 ? 0 : Math.max(300, latency * 2) * step), err);
    this.start = this.now();
    this.backupVariant = options.backup ?? 'mixed';
    this.backupCheckedAt = this.start - 40 * MIN;
    this.processes = buildProcesses(this.start, this.profile.specs, this.profile.rootPid);
    this.activityLog = demoActivity(this.start);
    this.alertLog = demoAlerts(this.start);
    this.snapshotList = [
      { id: 'snap-1', at: this.start - 3 * 60 * MIN, label: 'Before the build', processCount: this.processes.length - 7 },
      { id: 'snap-2', at: this.start - 20 * MIN, label: 'Morning baseline', processCount: this.processes.length - 2 },
    ];
    this.snapshotData.set('snap-1', this.samplesAt(this.start / 1000 - 3 * 3600).filter((s) => s.name !== this.profile.names.rustc && s.name !== this.profile.names.cargo));
    this.snapshotData.set('snap-2', this.samplesAt(this.start / 1000 - 1200).slice(0, -2));
  }

  // ---------------------------------------------------------------------------------------------- live data

  onLive(detail: LiveDetail, listener: (u: LiveUpdate) => void): Unsubscribe {
    this.liveListeners.set(listener, detail);
    setTimeout(() => {
      if (this.liveListeners.has(listener)) listener(this.liveUpdate(detail));
    }, 0);
    this.timer ??= setInterval(() => this.tick(), 1000);
    return () => {
      this.liveListeners.delete(listener);
      if (this.liveListeners.size === 0 && this.timer !== null) {
        clearInterval(this.timer);
        this.timer = null;
      }
    };
  }

  onAlert(listener: (a: Alert) => void): Unsubscribe {
    this.alertListeners.add(listener);
    return () => this.alertListeners.delete(listener);
  }

  onActivity(listener: (a: ActivityEntry) => void): Unsubscribe {
    this.activityListeners.add(listener);
    return () => this.activityListeners.delete(listener);
  }

  onAgentStatus(listener: (s: AgentStatus) => void): Unsubscribe {
    this.agentListeners.add(listener);
    setTimeout(() => {
      if (this.agentListeners.has(listener)) listener({ state: 'running', message: null });
    }, 0);
    return () => this.agentListeners.delete(listener);
  }

  /** One live tick (exposed for tests). */
  onAlarm(listener: (c: AlarmChange) => void): Unsubscribe {
    this.alarmListeners.add(listener);
    return () => this.alarmListeners.delete(listener);
  }

  tick(): void {
    const wantsProcesses = [...this.liveListeners.values()].includes('processes');
    const update = this.liveUpdate(wantsProcesses ? 'processes' : 'summary');
    for (const [listener, detail] of this.liveListeners) listener(detail === 'processes' || !wantsProcesses ? update : { ...update, processes: null, groups: null });
  }

  /** The live sample at the current second. */
  liveUpdate(detail: LiveDetail): LiveUpdate {
    const t = Math.floor(this.now() / 1000);
    const samples = this.samplesAt(t);
    const groups = groupSamples(this.processes, samples);
    const system = this.system(t, samples);
    const byCpu = [...groups].sort((a, b) => num(b.cpu) - num(a.cpu)).slice(0, 8);
    const byMemory = [...groups].sort((a, b) => num(b.memory) - num(a.memory)).slice(0, 8);
    return {
      system,
      pressure: system.memory.used / system.memory.total > 0.85 ? 'critical' : system.memory.used / system.memory.total > 0.7 ? 'elevated' : 'normal',
      topCpu: byCpu,
      topMemory: byMemory,
      overhead: {
        agentCpu: round(0.4 + 0.08 * wobble(t, 900, 5), 2),
        agentMemory: Math.round((38 + 0.6 * wobble(t, 901, 30)) * MB),
        samplingIntervalMs: this.settingsValue.sampling.intervalMs,
        processIntervalMs: this.settingsValue.sampling.processIntervalMs,
      },
      processes: detail === 'processes' ? samples : null,
      groups: detail === 'processes' ? groups : null,
      otherUsers: null,
    };
  }

  private samplesAt(t: number): ProcessSample[] {
    return this.processes.map((p) => sampleProcess(p, t, null, 1));
  }

  private system(t: number, samples: ProcessSample[]): SystemSample {
    const processCpu = samples.reduce((a, s) => a + num(s.cpu), 0);
    const total = clamp(processCpu + 1.4 + 0.6 * wobble(t, 500, 3), 0, 100);
    const perCore = Array.from({ length: this.profile.cores }, (_, i) => {
      const weight = this.kinds[i] === 'efficiency' ? 0.5 : this.kinds[i] === 'performance' ? 1.2 : 1;
      return round(clamp(total * weight * (1 + 0.4 * wobble(t, 600 + i, 3)) + (i === 0 ? 4 : 0), 0, 100), 1);
    });
    const coreFrequencyMhz = perCore.map((load, i) => {
      const e = this.kinds[i] === 'efficiency';
      const lo = e ? 1000 : this.profile.cpuBaseMhz + 400;
      const hi = e ? 2600 : this.profile.cpuMaxMhz;
      return Math.round(lo + ((hi - lo) * load) / 100);
    });
    const memTotal = this.profile.memoryTotal;
    // A slow swell on top of the processes' own memory (caches, compression) so the live line moves like a real machine's.
    const swell = (0.8 + 0.7 * wobble(t, 810, 22)) * GB + 0.18 * GB * wobble(t, 811, 3);
    const memUsed = Math.min(memTotal - 1.2 * GB, samples.reduce((a, s) => a + num(s.memory), 0) + 2.9 * GB + swell);
    const diskRead = samples.reduce((a, s) => a + num(s.diskReadBps), 0);
    const diskWrite = samples.reduce((a, s) => a + num(s.diskWriteBps), 0);
    const building = samples.some((s) => s.name === this.profile.names.rustc && num(s.cpu) > 1);
    const laptop = this.machine === 'laptop';
    return {
      at: t * 1000,
      cpu: { total: round(total, 1), perCore, coreKinds: [...this.kinds], coreFrequencyMhz, frequencyMhz: Math.round(this.profile.cpuMaxMhz - 260 + 260 * wobble(t, 700, 4)), baseFrequencyMhz: this.profile.cpuBaseMhz },
      memory: {
        total: memTotal,
        used: Math.round(memUsed),
        available: Math.round(memTotal - memUsed),
        commit: { available: { used: Math.round(memUsed + 6.4 * GB), limit: Math.round(memTotal + 24 * GB) } },
        swapUsed: Math.round(2.1 * GB),
        swapTotal: 24 * GB,
        pressure: { available: memUsed / memTotal > 0.85 ? 'critical' : memUsed / memTotal > 0.7 ? 'elevated' : 'normal' },
      },
      disks: this.profile.disks((this.now() - this.start) / DAY),
      diskIo: {
        readBps: diskRead,
        writeBps: diskWrite,
        queue: { available: round(clamp((building ? 4.2 : 0.3) + (building ? 0.9 : 0.1) * wobble(t, 800, 3), 0, 64), 1) },
        busy: { available: round(clamp((building ? 88 : 9) + (building ? 7 : 5) * wobble(t, 801, 3), 0, 100), 1) },
      },
      network: this.profile.network((salt, period) => wobble(t, salt, period)),
      gpu: {
        available: {
          name: this.profile.gpuName,
          utilization: round(clamp(9 + 6 * wobble(t, 820, 4), 0, 100), 1),
          memoryUsed: Math.round((2.3 + 0.1 * wobble(t, 821, 20)) * GB),
          memoryTotal: this.profile.os === 'macos' ? this.profile.memoryTotal : 8 * GB,
        },
      },
      battery: laptop
        ? { available: { charge: 86, source: 'ac', health: 87.2, cycles: 412, secondsToEmpty: null } }
        : { unavailable: this.profile.noBattery },
      temperatures: laptop
        ? {
            available: [
              { label: this.profile.cpuSensor, celsius: round(clamp(78 + 4 * wobble(t, 830, 6), 30, 105), 1), critical: 100 },
              { label: 'GPU', celsius: round(clamp(56 + 2 * wobble(t, 831, 8), 30, 100), 1), critical: 87 },
              { label: 'SSD', celsius: round(clamp(49 + 1.5 * wobble(t, 832, 10), 30, 90), 1), critical: 82 },
            ],
          }
        : { unavailable: this.profile.noTemperatures },
      fans:
        laptop && this.profile.os !== 'macos'
          ? {
              available: [
                { label: 'CPU fan', rpm: Math.round(4200 + 300 * wobble(t, 840, 6)) },
                { label: 'GPU fan', rpm: Math.round(3100 + 200 * wobble(t, 841, 6)) },
              ],
            }
          : { unavailable: this.profile.noFans },
      uptimeSecs: 12 * 86400 + 3 * 3600 + 14 * 60 + Math.floor((this.now() - this.start) / 1000),
    };
  }

  // ---------------------------------------------------------------------------------------------- processes

  processDetails(target: ProcessRef): Promise<ProcessDetails> {
    return this.reply('processDetails', () => {
      const p = this.find(target);
      const sample = sampleProcess(p, Math.floor(this.now() / 1000), null, 1);
      const parents: { pid: number; name: string }[] = [];
      for (let cur = p.parent; cur !== null; ) {
        const parent = this.processes.find((x) => x.pid === cur);
        if (!parent || parents.length > 12) break;
        parents.push({ pid: parent.pid, name: parent.name });
        cur = parent.parent;
      }
      const pr = this.profile;
      const system = p.system || p.exe === null;
      const browser = p.name === pr.names.chrome;
      const sep = pr.os === 'windows' ? '\\' : '/';
      const dir = p.exe ? p.exe.slice(0, Math.max(p.exe.lastIndexOf(sep), 0)) : null;
      const chromeData =
        pr.os === 'windows'
          ? `${pr.home}\\AppData\\Local\\Google\\Chrome\\User Data\\Default`
          : pr.os === 'macos'
            ? `${pr.home}/Library/Application Support/Google/Chrome/Default`
            : `${pr.home}/.config/google-chrome/Default`;
      return {
        process: sample,
        commandLine: this.commandLine(p),
        cwd: system ? null : dir,
        priority: p.group === 'cargo' ? 'belowNormal' : 'normal',
        affinity: pr.os === 'macos' ? null : 2 ** pr.cores - 1,
        efficiencyMode: false,
        threads: Math.round(8 + (p.memory / MB) * 0.08),
        handles: Math.round(180 + (p.memory / MB) * 1.4),
        parents,
        signature: system
          ? { signed: pr.signer }
          : p.name === pr.names.rustc || p.name === pr.names.cargo
            ? 'unsigned'
            : { signed: publisher(p, pr.signer) },
        openFiles: system
          ? { unavailable: pr.openFilesDenied }
          : {
              items: {
                items: browser
                  ? [`${chromeData}${sep}History`, `${chromeData}${sep}Cookies`, `${chromeData}${sep}Cache${sep}Cache_Data${sep}data_1`]
                  : [`${p.exe ?? ''}`, pr.libs[0] ?? '', `${pr.projectDir}${sep}target${sep}debug${sep}deps${sep}dyno_core-4f2a.rlib`],
                truncated: false,
              },
            },
        connections: system
          ? { unavailable: 'Needs administrator rights for system processes.' }
          : {
              items: {
                items: browser
                  ? [
                      { protocol: 'TCP', local: '192.168.1.42:51044', remote: '142.250.185.78:443', state: 'Established' },
                      { protocol: 'TCP', local: '192.168.1.42:51049', remote: '140.82.112.25:443', state: 'Established' },
                      { protocol: 'UDP', local: '0.0.0.0:5353', remote: '*:*', state: '' },
                    ]
                  : [],
                truncated: false,
              },
            },
        modules: system ? { unavailable: pr.modulesDenied } : { items: { items: pr.libs, truncated: true } },
        startupSource: p.group === 'teams' ? 'Login item: Microsoft Teams' : p.group === 'docker' ? 'Login item: VirtualBox' : null,
      };
    });
  }

  private commandLine(p: MockProcess): string[] {
    const exe = p.exe ?? p.name;
    const n = this.profile.names;
    const src = this.profile.os === 'windows' ? 'crates\\dyno-core\\src\\lib.rs' : 'crates/dyno-core/src/lib.rs';
    switch (p.name) {
      case n.chrome:
        return [exe, '--type=renderer', '--lang=en-GB', '--renderer-client-id=42'];
      case n.rustc:
        return [exe, '--crate-name', 'dyno_core', '--edition=2024', src, '--crate-type', 'lib', '-C', 'opt-level=3'];
      case n.cargo:
        return [exe, 'build', '--release', '--workspace'];
      case 'svchost.exe':
        return [exe, '-k', 'netsvcs', '-p'];
      case n.node:
        return [exe, 'node_modules/@angular/cli/bin/ng.js', 'serve'];
      default:
        return [exe];
    }
  }

  lockers(path: string): Promise<Locker[]> {
    return this.reply('lockers', () => {
      if (!path.trim()) throw err('badRequest', 'Enter a file or folder path.');
      const lower = path.toLowerCase();
      const pick = (name: string, kind: string | null) => {
        const p = this.processes.find((x) => x.name === name);
        return p ? [{ process: { pid: p.pid, started: p.started }, name: p.name, kind }] : [];
      };
      if (lower.includes('target')) return [...pick(this.profile.names.rustc, 'Writes build output'), ...pick(this.profile.names.code, 'rust-analyzer watches it'), ...pick(this.profile.names.indexer, 'Indexing')];
      if (lower.includes('chrome')) return pick(this.profile.names.chrome, 'Browser profile');
      if (lower.endsWith('.log')) return pick(this.profile.names.agent, null);
      return [];
    });
  }

  openLocation(target: ProcessRef): Promise<void> {
    return this.reply('openLocation', () => {
      const p = this.find(target);
      if (!p.exe) throw err('unsupported', `${p.name} has no file on disk (${this.profile.kernelNote}).`);
    });
  }

  takeSnapshot(label: string): Promise<SnapshotInfo> {
    return this.reply('takeSnapshot', () => {
      const samples = this.samplesAt(Math.floor(this.now() / 1000));
      const info: SnapshotInfo = { id: `snap-${this.nextId++}`, at: this.now(), label: label.trim() || 'Snapshot', processCount: samples.length };
      this.snapshotList = [info, ...this.snapshotList];
      this.snapshotData.set(info.id, samples);
      return info;
    });
  }

  snapshots(): Promise<SnapshotInfo[]> {
    return this.reply('snapshots', () => [...this.snapshotList].sort((a, b) => b.at - a.at));
  }

  diffSnapshots(before: string, after: string): Promise<SnapshotDiff> {
    return this.reply('diffSnapshots', () => {
      const b = this.snapshotList.find((s) => s.id === before);
      const a = this.snapshotList.find((s) => s.id === after);
      if (!a || !b) throw err('notFound', 'That snapshot was deleted.');
      const [first, second] = b.at <= a.at ? [b, a] : [a, b];
      const x = this.snapshotData.get(first.id) ?? [];
      const y = this.snapshotData.get(second.id) ?? [];
      const xs = new Map(x.map((s) => [s.pid, s]));
      const ys = new Map(y.map((s) => [s.pid, s]));
      const summary = (s: ProcessSample) => ({ pid: s.pid, name: s.name, exe: s.exe, cpu: s.cpu, memory: s.memory });
      const changed = y
        .filter((s) => xs.has(s.pid))
        .map((s) => ({ pid: s.pid, name: s.name, cpuBefore: num(xs.get(s.pid)!.cpu), cpuAfter: num(s.cpu), memoryBefore: num(xs.get(s.pid)!.memory), memoryAfter: num(s.memory) }))
        .filter((c) => Math.abs(c.memoryAfter - c.memoryBefore) >= 10 * MB || Math.abs(c.cpuAfter - c.cpuBefore) >= 5)
        .sort((p, q) => Math.abs(q.memoryAfter - q.memoryBefore) - Math.abs(p.memoryAfter - p.memoryBefore));
      const sum = (l: ProcessSample[], f: (s: ProcessSample) => number) => l.reduce((acc, s) => acc + f(s), 0);
      return {
        before: first,
        after: second,
        added: y.filter((s) => !xs.has(s.pid)).map(summary),
        removed: x.filter((s) => !ys.has(s.pid)).map(summary),
        changed,
        memoryUsedDelta: Math.round(sum(y, (s) => num(s.memory)) - sum(x, (s) => num(s.memory))),
        cpuDelta: round(sum(y, (s) => num(s.cpu)) - sum(x, (s) => num(s.cpu)), 1),
      };
    });
  }

  // ---------------------------------------------------------------------------------- diagnosis and history

  whySlow(): Promise<Diagnosis> {
    return this.reply('whySlow', () => {
      const now = this.now();
      const t = Math.floor(now / 1000);
      const samples = this.samplesAt(t);
      const rustc = samples.filter((s) => s.name === this.profile.names.rustc);
      const rustcCpu = rustc.reduce((a, s) => a + num(s.cpu), 0);
      const io = this.system(t, samples).diskIo;
      const queue = 'available' in io.queue ? io.queue.available : 0;
      if (rustc.length === 0 || rustcCpu < 5) {
        return { at: now, windowSecs: 55, headline: 'Nothing is slowing your computer down right now', causes: [{ kind: 'nothingWrong', title: 'All quiet', explanation: 'CPU, memory and disk all have room to spare.', confidence: 0.9, evidence: [], hints: [], involved: [] }] };
      }
      return {
        at: now,
        windowSecs: 55,
        headline: 'A Rust build is using most of your CPU',
        causes: [
          {
            kind: 'cpuHog',
            title: `${this.profile.names.rustc} is using ${Math.round(rustcCpu)}% CPU`,
            explanation: 'cargo is compiling dyno-core with 6 parallel rustc processes. Everything else waits for a free core; it ends when the build finishes.',
            confidence: 0.86,
            evidence: [
              { label: `${this.profile.names.rustc} (6)`, value: `${Math.round(rustcCpu)}% CPU` },
              { label: 'CPU total', value: `${Math.round(this.system(t, samples).cpu.total)}%` },
              { label: 'For', value: '6 min' },
            ],
            hints: [{ text: 'Open Processes', link: 'taskManager' }],
            involved: [
              { id: this.profile.names.rustc, name: this.profile.names.rustc, share: `${Math.round(rustcCpu)}% CPU` },
              { id: this.profile.names.cargo, name: this.profile.names.cargo, share: '2% CPU' },
            ],
          },
          {
            kind: 'diskQueue',
            title: 'The disk is queueing writes',
            explanation: 'rustc writes 180 MB/s of build output; requests wait in line, so apps that open files feel sluggish.',
            confidence: 0.58,
            evidence: [
              { label: 'Disk queue', value: queue.toFixed(1) },
              { label: 'Disk busy', value: '92%' },
              { label: 'Writes', value: `121 MB/s by ${this.profile.names.rustc}` },
            ],
            hints: [{ text: 'Wait for the build to finish', link: null }],
            involved: [{ id: this.profile.names.rustc, name: this.profile.names.rustc, share: '121 MB/s' }],
          },
          {
            kind: 'antivirusScan',
            title: 'Defender scans every file the build writes',
            explanation: 'Microsoft Defender inspects build output as it is written. Excluding build folders (not source) avoids it.',
            confidence: 0.41,
            evidence: [
              { label: this.profile.names.indexer, value: '4.1% CPU' },
              { label: 'Scanning', value: `${this.profile.projectDir}${this.profile.os === 'windows' ? '\\' : '/'}target` },
            ],
            hints: [{ text: 'See what starts at sign-in', link: 'startup' }],
            involved: [{ id: this.profile.names.indexer, name: this.profile.names.indexer, share: '4% CPU' }],
          },
          {
            kind: 'lowDiskSpace',
            title: 'Drive C: is 78% full',
            explanation: 'Not slowing you down yet, but Windows needs free space for updates and swap.',
            confidence: 0.22,
            evidence: [{ label: 'Free', value: '210 GB of 953 GB' }],
            hints: [],
            involved: [],
          },
        ],
      };
    });
  }

  anomalies(): Promise<Anomaly[]> {
    return this.reply('anomalies', () => demoAnomalies(this.start));
  }

  history(range: HistoryRange, end: Timestamp | null): Promise<History> {
    return this.reply('history', () => {
      const h = mockHistory(range, end, this.now(), this.profile.cores, this.kinds);
      if (this.profile.os !== 'windows') {
        // Only the disks this machine has, under their names (the shared demo data has two Windows drives).
        const disks = this.profile.disks(0);
        h.diskTrends = h.diskTrends.slice(0, Math.max(1, disks.length));
        h.diskTrends.forEach((t, i) => (t.disk = i === 0 ? this.profile.primaryDisk : (disks[i]?.name ?? t.disk)));
      }
      return h;
    });
  }

  activity(limit: number): Promise<ActivityEntry[]> {
    return this.reply('activity', () => this.activityLog.slice(0, Math.max(0, limit)));
  }

  // ------------------------------------------------------------------------------------------------- rules

  /** The mock's volumes never change. */
  onDiskVolumes(_listener: (v: Volume[]) => void): Unsubscribe {
    return () => undefined;
  }

  onDiskScan(listener: (s: DiskScanStatus) => void): Unsubscribe {
    return this.disk.onStatus(listener);
  }

  diskVolumes(): Promise<Volume[]> {
    return this.reply('diskVolumes', () => this.disk.volumes());
  }

  startDiskScan(target: DiskTarget): Promise<DiskScanStatus> {
    return this.reply('startDiskScan', () => this.disk.start(target));
  }

  cancelDiskScan(scan: number): Promise<void> {
    return this.reply('cancelDiskScan', () => this.disk.cancel(scan));
  }

  diskScans(): Promise<DiskScanStatus[]> {
    return this.reply('diskScans', () => this.disk.list());
  }

  diskChildren(query: DiskChildrenQuery): Promise<DiskChildren> {
    return this.reply('diskChildren', () => this.disk.children(query));
  }

  diskRings(query: DiskRingsQuery): Promise<DiskRings> {
    return this.reply('diskRings', () => this.disk.rings(query));
  }

  diskPath(scan: number, node: number): Promise<string> {
    return this.reply('diskPath', () => this.disk.path(scan, node));
  }

  retryDiskScan(scan: number): Promise<DiskScanStatus> {
    return this.reply('retryDiskScan', () => this.disk.retry(scan));
  }

  openFullDiskAccessSettings(): Promise<void> {
    return this.reply('openFullDiskAccessSettings', () => undefined);
  }

  diskReveal(scan: number, node: number): Promise<void> {
    return this.reply('diskReveal', () => this.disk.reveal(scan, node));
  }

  // ------------------------------------------------------------------------------------- health

  health(): Promise<HealthReport> {
    return this.reply('health', () => {
      const reasons = healthReasons();
      const score = 100 - reasons.reduce((a, r) => a + r.impact, 0);
      return { at: this.now(), score, grade: score >= 80 ? 'good' : score >= 60 ? 'fair' : 'poor', reasons };
    });
  }

  startupEntries(): Promise<StartupEntry[]> {
    return this.reply('startupEntries', () => this.startup);
  }

  // ------------------------------------------------------------------------------- reports, settings, plugins

  inventory(): Promise<SystemInventory> {
    return this.reply('inventory', () => mockInventory(this.profile.os));
  }

  counterObjects(): Promise<CounterObjectList> {
    return this.reply('counterObjects', () => this.counters.objects());
  }

  counterObject(name: string): Promise<CounterObject> {
    return this.reply('counterObject', () => this.counters.object(name));
  }

  counterSearch(query: string, limit: number): Promise<CounterHit[]> {
    return this.reply('counterSearch', () => this.counters.search(query, limit));
  }

  counterValues(paths: string[]): Promise<CounterReading[]> {
    return this.reply('counterValues', () => this.counters.values(paths, this.now()));
  }

  counterHistory(paths: string[], minutes: number): Promise<CounterHistory> {
    return this.reply('counterHistory', () => this.counters.history(paths, minutes, this.now()));
  }

  throttling(): Promise<ThrottleOverview> {
    return this.reply('throttling', () => mockThrottling(this.profile.os, this.now()));
  }

  lifespan(): Promise<LifespanOverview> {
    return this.reply('lifespan', () => mockLifespan(this.profile.os, this.machine));
  }

  baselines(): Promise<BaselineOverview> {
    return this.reply('baselines', () => mockBaselines(this.now()));
  }

  network(query: NetworkQuery): Promise<NetworkSnapshot> {
    return this.reply('network', () => mockNetwork(query, this.now()));
  }

  resolveHosts(addresses: string[]): Promise<HostName[]> {
    return this.reply('resolveHosts', () => {
      if (addresses.length > 64) throw err('badRequest', 'Resolve at most 64 addresses at a time.');
      return addresses.map((address) => ({ address, name: mockHostName(address) }));
    });
  }

  // -------------------------------------------------------------------------------------------- alarms

  private alarmRows(): Alarm[] {
    return (this.alarmList ??= demoAlarms(this.now()));
  }

  private changeAlarm(ids: number[], kind: AlarmChange['kind'], edit: (a: Alarm) => Alarm): Alarm[] {
    const at = this.now();
    const changed: Alarm[] = [];
    this.alarmList = this.alarmRows().map((a) => {
      if (!ids.includes(a.id)) return a;
      const next = edit(a);
      changed.push(next);
      return next;
    });
    if (changed.length === 0) throw err('notFound', 'That alarm is gone.');
    for (const alarm of changed) for (const l of this.alarmListeners) l({ kind, alarm: copy(alarm), notify: false, at });
    return copy(changed);
  }

  alarms(filter: AlarmFilter): Promise<Alarm[]> {
    return this.reply('alarms', () => copy(filterAlarms(this.alarmRows(), filter, this.now())));
  }

  alarmStats(): Promise<AlarmStats> {
    return this.reply('alarmStats', () => demoStats(this.alarmRows()));
  }

  acknowledgeAlarms(ids: number[]): Promise<Alarm[]> {
    return this.reply('acknowledgeAlarms', () => {
      if (ids.length === 0 || ids.length > 500) throw err('badRequest', 'Choose between 1 and 500 alarms to acknowledge.');
      const at = this.now();
      return this.changeAlarm(ids, 'acknowledged', (a) => (a.state === 'active' ? { ...a, state: 'acknowledged', acknowledgedAt: at } : a.state === 'clearedUnacknowledged' ? { ...a, state: 'cleared', acknowledgedAt: at } : a));
    });
  }

  shelveAlarm(id: number, until: ShelveFor): Promise<Alarm[]> {
    return this.reply('shelveAlarm', () => {
      const at = this.now();
      const shelf = until === 'untilReboot' ? ('untilReboot' as const) : { until: typeof until === 'object' ? until.custom : until === 'oneHour' ? at + 60 * MIN : at + 12 * 60 * MIN };
      if (typeof shelf === 'object' && shelf.until <= at) throw err('badRequest', 'Choose a time in the future to shelve the alarm until.');
      return this.changeAlarm([id], 'shelved', (a) => ({ ...a, shelved: shelf }));
    });
  }

  unshelveAlarm(id: number): Promise<Alarm[]> {
    return this.reply('unshelveAlarm', () => this.changeAlarm([id], 'unshelved', (a) => ({ ...a, shelved: null })));
  }

  muteRule(rule: string, muted: boolean): Promise<Alarm[]> {
    return this.reply('muteRule', () => {
      const ids = this.alarmRows().filter((a) => a.rule === rule).map((a) => a.id);
      if (ids.length === 0) return [];
      return this.changeAlarm(ids, 'muted', (a) => ({ ...a, muted }));
    });
  }

  noteAlarm(id: number, text: string): Promise<Alarm[]> {
    return this.reply('noteAlarm', () => {
      const t = text.trim();
      if (!t || t.length > 200) throw err('badRequest', 'Write a note of 1 to 200 characters.');
      const at = this.now();
      return this.changeAlarm([id], 'noted', (a) => ({ ...a, notes: [...a.notes, { at, text: t }] }));
    });
  }

  alarmHistory(alarmId: number | null, limit: number): Promise<AlarmChange[]> {
    return this.reply('alarmHistory', () => copy(demoHistory(this.alarmRows()).filter((c) => alarmId === null || c.alarm.id === alarmId).slice(0, Math.min(500, Math.max(1, limit)))));
  }

  /** Whether the mock treats the alarm as shelved now (for tests). */
  isShelved(a: Alarm): boolean {
    return isShelved(a, this.now());
  }

  // ------------------------------------------------------------------------------ recorder, diagnostics

  private snapshotRows(): RecorderSnapshotInfo[] {
    return (this.snapshotInfos ??= demoSnapshotInfos(this.now()));
  }

  capture(): Promise<RecorderSnapshotInfo> {
    return this.reply('capture', () => {
      const info: RecorderSnapshotInfo = { id: Math.max(0, ...this.snapshotRows().map((x) => x.id)) + 1, at: this.now(), reason: 'manual', bytes: 400_000, samples: 600 };
      this.snapshotInfos = [info, ...this.snapshotRows()];
      return info;
    });
  }

  recorderSnapshots(): Promise<RecorderSnapshotInfo[]> {
    return this.reply('recorderSnapshots', () => copy(this.snapshotRows()));
  }

  recorderSnapshot(id: number): Promise<RecorderSnapshot> {
    return this.reply('recorderSnapshot', () => {
      const info = this.snapshotRows().find((x) => x.id === id);
      if (!info) throw err('notFound', 'That snapshot was deleted.');
      return demoSnapshot(info);
    });
  }

  diagnosticsPreview(): Promise<BundlePreview> {
    return this.reply('diagnosticsPreview', () => demoBundlePreview());
  }

  saveDiagnostics(): Promise<SavedDiagnostics | null> {
    return this.reply('saveDiagnostics', () => {
      const p = demoBundlePreview();
      const saved = { path: '/Users/me/dyno-diagnostics.zip', bytes: Math.round(p.totalBytes * 0.22), files: p.files.length };
      this.savedBundles.push(saved);
      return saved;
    });
  }

  /** The URLs the mock "opened", for tests. */
  readonly opened: string[] = [];

  lookupIp(ip: string, template?: string): Promise<void> {
    return this.reply('lookupIp', () => {
      const tpl = template ?? this.settingsValue.ipLookup?.ipTemplate ?? 'https://ipinfo.io/{ip}';
      if (!tpl.startsWith('https://') || !tpl.includes('{ip}')) throw err('badRequest', 'The lookup address must start with https:// and contain {ip}.');
      if (/^(10\.|127\.|192\.168\.|169\.254\.|100\.(6[4-9]|[7-9]\d|1[01]\d|12[0-7])\.)/.test(ip) || ip === '::1') throw err('badRequest', 'Private address, nothing to look up.');
      this.opened.push(tpl.replace('{ip}', ip));
    });
  }

  searchSeed(text: string): Promise<SearchSeed> {
    return this.reply('searchSeed', () => mockSearchSeed(text));
  }

  webSearch(query: string): Promise<void> {
    return this.reply('webSearch', () => {
      try {
        this.opened.push(mockSearchUrl(query));
      } catch (e) {
        throw err('badRequest', (e as Error).message);
      }
    });
  }

  openVirusTotal(sha256: string): Promise<void> {
    return this.reply('openVirusTotal', () => {
      if (!/^[0-9a-f]{64}$/i.test(sha256)) throw err('badRequest', 'A SHA-256 hash has 64 hexadecimal digits.');
      this.opened.push(`https://www.virustotal.com/gui/file/${sha256.toLowerCase()}`);
    });
  }

  startupOverview(filter: StartupFilter): Promise<StartupOverview> {
    return this.reply('startupOverview', () => copy(mockStartupOverview(this.startup, filter)));
  }

  /** Turns a plain Error from the pure mock helpers into the contract's bad-request error. */
  private guard<T>(body: () => T): T {
    try {
      return body();
    } catch (e) {
      if (e instanceof Error) throw err('badRequest', e.message);
      throw e;
    }
  }

  private benchRuns: BenchRun[] | null = null;
  private benchRunning: BenchProgress | null = null;

  private runs(): BenchRun[] {
    return (this.benchRuns ??= [
      mockRun('r3', this.start - 2 * 60 * MIN, `${osTerms(this.profile.os).powerExample} (after)`, 1.12, { group: osTerms(this.profile.os).powerExample, phase: 'after' }, this.profile.cores),
      mockRun('r2', this.start - 3 * 60 * MIN, `${osTerms(this.profile.os).powerExample} (before)`, 1, { group: osTerms(this.profile.os).powerExample, phase: 'before' }, this.profile.cores),
      mockRun('r1', this.start - 2 * DAY, 'Baseline', 0.99, { load: { cpuPercent: 31, memoryPercent: 61, onBattery: false } }, this.profile.cores),
    ]);
  }

  benchmarks(): Promise<BenchOverview> {
    return this.reply('benchmarks', () => ({ running: this.benchRunning, runs: this.runs() }));
  }

  startBenchmark(r: StartBenchmark): Promise<BenchOverview> {
    return this.reply('startBenchmark', () => {
      if (this.benchRunning) throw err('busy', 'A benchmark is already running; wait for it or cancel it.');
      if (!r.kinds.length) throw err('badRequest', 'Choose at least one benchmark to run.');
      const kinds = [...new Set(r.kinds)];
      const progress: BenchProgress = { runId: `r${this.runs().length + 1}`, startedAt: Date.now(), kinds, current: kinds[0] ?? null, done: [] };
      this.benchRunning = progress;
      const step = (i: number) => {
        const p = this.benchRunning;
        if (!p || p.runId !== progress.runId) return;
        if (i >= kinds.length) {
          const run = mockRun(p.runId, this.start, r.label || 'Benchmark', r.phase === 'after' ? 1.12 : 1, {
            group: r.group ?? null,
            phase: r.phase ?? null,
            results: kinds.map((k) => mockKindResult(k, r.phase === 'after' ? 1.12 : 1, false, this.profile.cores)),
          }, this.profile.cores);
          this.runs().unshift(run);
          this.benchRunning = null;
          return;
        }
        const kind = kinds[i]!;
        p.current = kind;
        setTimeout(() => {
          p.done.push(kind);
          p.current = null;
          step(i + 1);
        }, 900);
      };
      step(0);
      return { running: this.benchRunning, runs: this.runs() };
    });
  }

  cancelBenchmark(): Promise<void> {
    return this.reply('cancelBenchmark', () => {
      this.benchRunning = null;
    });
  }

  deleteBenchmark(id: string): Promise<void> {
    return this.reply('deleteBenchmark', () => {
      const i = this.runs().findIndex((x) => x.id === id);
      if (i < 0) throw err('notFound', 'That benchmark run no longer exists.');
      this.runs().splice(i, 1);
    });
  }

  compareBenchmarks(before: string, after: string): Promise<BenchComparison> {
    return this.reply('compareBenchmarks', () => {
      const b = this.runs().find((x) => x.id === before);
      const a = this.runs().find((x) => x.id === after);
      if (!a || !b) throw err('notFound', 'That benchmark run no longer exists.');
      return mockCompare(b, a);
    });
  }

  weeklyReport(weeksAgo: number): Promise<WeeklyReport> {
    return this.reply('weeklyReport', () => {
      if (weeksAgo < 0 || weeksAgo > 52) throw err('badRequest', 'Dyno keeps at most 52 weeks; choose a more recent one.');
      const to = this.start - weeksAgo * 7 * DAY;
      const from = to - 7 * DAY;
      // Dyno has been collecting for about five weeks: older weeks are empty and say so.
      if (weeksAgo >= 5) {
        return { from, to, score: null, scoreChange: 0, avgCpu: 0, peakCpu: 0, avgMemory: 0, topApps: [], crashes: 0, alerts: 0, highlights: ['Dyno has no history for this week yet.'], scoreTrend: [], disks: [], issues: [], hardware: [], nothingToReport: true };
      }
      const issueAt = (h: number): number => to - h * 60 * MIN;
      return {
        from,
        to,
        score: 74 - weeksAgo,
        scoreChange: 3,
        avgCpu: 17.4,
        peakCpu: 99,
        avgMemory: 58.2,
        topApps: mockHistory('week', null, this.now()).topConsumers.slice(0, 5),
        crashes: 9,
        alerts: 2,
        highlights: [
          'Your computer averaged 17% CPU and 58% memory, peaking at 99% CPU.',
          'The VirtualBox service crashed 6 times this week: check its virtual machine settings.',
          'Sign-in takes 9 s longer than a month ago: 2 new startup apps.',
        ],
        scoreTrend: [66, 68, 71, 70, 72, 74].map((score, i) => ({ at: from + i * 28 * 60 * MIN, score })),
        disks: [
          { disk: this.profile.os === 'windows' ? 'C:' : '/', total: 953 * GB, free: 210 * GB, forecast: { fullInDays: 41 } },
        ],
        issues: [
          { at: issueAt(6 * 60), severity: 'warning', title: 'Drive is getting full', detail: '210 GB free of 953 GB; at this rate it is full in about 41 days.' },
          { at: issueAt(30 * 60), severity: 'info', title: 'A build used a lot of CPU for a long time', detail: 'rustc averaged 74% for 32 minutes.' },
        ],
        hardware: ['The CPU ran slower than it should for 52 min this week (52 min of heat).', 'Battery: replace in about 23 months (17 to 31 months) (confidence medium).'],
        nothingToReport: false,
      };
    });
  }

  /** The reports the mock "saved", for tests. */
  readonly savedReports: { format: ReportFormat; weeksAgo: number }[] = [];

  issues(query: IssuesQuery): Promise<IssuesOverview> {
    return this.reply('issues', () => mockIssuesOverview(query, this.now(), mockSources(this.now())));
  }

  saveIssues(days: number): Promise<SavedIssues | null> {
    return this.reply('saveIssues', () => {
      this.savedIssues.push(days);
      return { path: '/Users/me/dyno-issues.jsonl', bytes: 3_412, count: 13 };
    });
  }

  unusual(): Promise<UnusualOverview> {
    return this.reply('unusual', () => this.unusualView());
  }

  forgetBaseline(): Promise<UnusualOverview> {
    return this.reply('forgetBaseline', () => {
      this.unusualItems = [];
      this.unusualForgotten = true;
      return this.unusualView();
    });
  }

  private unusualView(): UnusualOverview {
    const view = mockUnusual(this.unusualItems, this.now(), 2);
    if (!this.unusualForgotten) return view;
    return { ...view, known: 0, learning: { active: true, until: this.now() + this.settingsValue.unusual!.learningDays * DAY } };
  }

  saveReport(format: ReportFormat, weeksAgo: number): Promise<SavedReport | null> {
    return this.reply('saveReport', () => {
      this.savedReports.push({ format, weeksAgo });
      return { path: `/Users/me/dyno-weekly-report.${format === 'html' ? 'html' : 'md'}`, bytes: 2_048 };
    });
  }

  alerts(limit: number): Promise<Alert[]> {
    return this.reply('alerts', () => this.alertLog.slice(0, Math.max(0, limit)));
  }

  settings(): Promise<Settings> {
    return this.reply('settings', () => this.settingsValue);
  }

  saveSettings(settings: Settings): Promise<Settings> {
    return this.reply('saveSettings', () => {
      if (settings.sampling.intervalMs < 250) throw err('badRequest', 'Sample at most 4 times a second (250 ms).');
      if (settings.webApi.port < 1024 || settings.webApi.port > 65535) throw err('badRequest', 'Choose a port between 1024 and 65535.');
      const extras = mockMaintenanceProblem(settings.maintenance) ?? mockBackupProblem(settings.backup);
      if (extras) throw err('badRequest', extras);
      this.settingsValue = copy(settings);
      return this.settingsValue;
    });
  }

  upsertAlertRule(rule: CustomAlertRule): Promise<Settings> {
    return this.reply('upsertAlertRule', () => {
      if (!rule.name.trim()) throw err('badRequest', 'Give the rule a name.');
      const rules = this.settingsValue.customAlerts ?? [];
      const exists = rules.some((r) => r.id === rule.id);
      this.settingsValue = copy({ ...this.settingsValue, customAlerts: exists ? rules.map((r) => (r.id === rule.id ? rule : r)) : [...rules, rule] });
      return this.settingsValue;
    });
  }

  deleteAlertRule(id: string): Promise<Settings> {
    return this.reply('deleteAlertRule', () => {
      this.settingsValue = copy({ ...this.settingsValue, customAlerts: (this.settingsValue.customAlerts ?? []).filter((r) => r.id !== id) });
      return this.settingsValue;
    });
  }

  plugins(): Promise<PluginInfo[]> {
    return this.reply('plugins', () => demoPlugins());
  }

  // ----------------------------------------------------------------------------------------- widget layouts

  widgetLayout(surface: DynoSurface): Promise<SurfaceView> {
    return this.reply('widgetLayout', () => this.widgets.view(surface));
  }

  editWidgetLayout(edit: SurfaceEdit): Promise<SurfaceView> {
    return this.reply('editWidgetLayout', () => this.widgets.edit(edit));
  }

  popoverValues(): Promise<PopoverSurfaceValues> {
    return this.reply('popoverValues', () => this.widgets.values());
  }

  popoverPreview(): Promise<PopoverSurfaceValues> {
    return this.reply('popoverPreview', () => this.widgets.values());
  }

  popoverSnapshot(): Promise<PopoverSurfaceValues> {
    return this.reply('popoverSnapshot', () => this.widgets.values());
  }

  runWidgetAction(action: WidgetActionId): Promise<void> {
    this.widgetActions.push(action);
    return this.reply('runWidgetAction', () => undefined);
  }

  /** The widget buttons pressed so far, for tests. */
  readonly widgetActions: WidgetActionId[] = [];

  onPopoverValues(listener: (v: PopoverSurfaceValues) => void): Unsubscribe {
    return this.widgets.onValues(listener);
  }

  onWidgetLayout(listener: (surface: DynoSurface) => void): Unsubscribe {
    return this.widgets.onLayout(listener);
  }

  private pendingNavigation: Navigate | null = null;
  private readonly navigateListeners = new Set<(to: Navigate) => void>();

  onNavigate(listener: (to: Navigate) => void): Unsubscribe {
    this.navigateListeners.add(listener);
    return () => this.navigateListeners.delete(listener);
  }

  /** Asks the main window to open a page or a Settings section, as a popup widget's button does (tests). */
  navigate(to: Navigate): void {
    this.pendingNavigation = to;
    for (const l of this.navigateListeners) l(to);
  }

  /** Pretends to open the taskbar settings (the mock has no OS). */
  openTaskbarSettings(): Promise<void> {
    return this.reply('openTaskbarSettings', () => undefined);
  }

  /** Hands the waiting navigation over, once. */
  takeNavigation(): Promise<Navigate | null> {
    return this.reply('takeNavigation', () => {
      const to = this.pendingNavigation;
      this.pendingNavigation = null;
      return to;
    });
  }

  // The mock imitates the contract with plausible data; the real readings, tones and choices come from the engine.
  metricReadings(queries: ReadingQuery[]): Promise<MetricReading[]> {
    return this.reply('metricReadings', () => {
      const sys = this.liveUpdate('summary').system;
      const memory = sys.memory.total > 0 ? (sys.memory.used / sys.memory.total) * 100 : 0;
      const battery = 'available' in sys.battery ? sys.battery.available.charge : sys.battery.unavailable;
      const gpu = 'available' in sys.gpu ? sys.gpu.available.utilization : sys.gpu.unavailable;
      const diskBusy = 'available' in sys.diskIo.busy ? sys.diskIo.busy.available : sys.diskIo.busy.unavailable;
      const disk = sys.disks[0];
      const rxBps = sys.network.reduce((n, i) => n + i.rxBps, 0);
      const txBps = sys.network.reduce((n, i) => n + i.txBps, 0);
      return queries.map((q) =>
        mockMetricReading(q, { cpu: sys.cpu.total, memory, battery, gpu, diskBusy, diskFree: disk && disk.total > 0 ? (disk.free / disk.total) * 100 : undefined, rxBps, txBps }),
      );
    });
  }

  widgetChoices(): Promise<WidgetChoices> {
    return this.reply('widgetChoices', () => mockWidgetChoices());
  }

  showMainWindow(): Promise<void> {
    return this.reply('showMainWindow', () => undefined);
  }

  fitTray(_height: number): Promise<void> {
    return Promise.resolve();
  }

  hideTray(): Promise<void> {
    return this.reply('hideTray', () => undefined);
  }

  quitApp(): Promise<void> {
    return this.reply('quitApp', () => undefined);
  }

  // ----------------------------------------------------------------------------------------- extras, backup

  changes(query: ChangesQuery): Promise<ChangeTimeline> {
    return this.reply('changes', () => mockChanges(query, this.now()));
  }

  gpuProcesses(): Promise<GpuReport> {
    return this.reply('gpuProcesses', () => mockGpu());
  }

  saveInventoryReport(format: InventoryFormat): Promise<SavedInventory | null> {
    return this.reply('saveInventoryReport', () => {
      this.savedInventories.push(format);
      return { path: `/Users/alex/Documents/dyno-inventory.${format === 'markdown' ? 'md' : 'json'}`, bytes: 18_204 };
    });
  }

  pickFolder(): Promise<string | null> {
    return this.reply('pickFolder', () => {
      this.pickedFolders++;
      return '/Users/alex/Projects';
    });
  }

  maintenance(): Promise<MaintenanceStatus> {
    return this.reply('maintenance', () => mockMaintenanceStatus(this.settingsValue.maintenance, this.now()));
  }

  backup(): Promise<BackupReport> {
    return this.reply('backup', () => mockBackupReport(this.backupVariant, this.now(), this.backupCheckedAt));
  }

  // ----------------------------------------------------------------------------------------------- jobs

  /**
   * How many steps a job the mock starts takes before it ends (1: it ends on the first). The mock plays the agent's part: every
   * step is pushed to the job sink as the shell's `paddock://jobs` event would be, and the shell's jobs store follows it. A spec
   * raises this to watch the progress and Cancel.
   */
  jobSteps = 1;
  /** Milliseconds between two steps (0 with no latency; the browser build shows the progress at a visible pace). */
  jobStepMs: number;
  /** A job the mock starts reports its first status and then nothing more (a step that takes long): the card says "Still working". */
  jobsStalled = false;
  private readonly jobSink: MockJobSink | undefined;
  private readonly jobBook = new Map<number, MockJob>();
  private nextJobId = 1;

  /** The latest status of every job the mock knows, oldest first. */
  jobStatuses(): SuJobSnapshot[] {
    return [...this.jobBook.values()].map((j) => j.status);
  }

  startJob(spec: JobSpec): Promise<number> {
    return this.reply('startJob', () => {
      const kind = jobKind(spec);
      const live = [...this.jobBook.values()].filter((j) => isActive(j.status));
      const same = live.find((j) => JSON.stringify(j.spec) === JSON.stringify(spec));
      if (same) return same.status.id;
      if (kind !== 'file-hash' && live.some((j) => j.status.kind === kind)) throw err('busy', `${JOB_TITLES[kind]} is already running; wait for it or cancel it.`);
      const o = objectOf(spec);
      if ('findFiles' in o && !(o as { findFiles: { query: FinderQuery } }).findFiles.query.folder.trim()) throw err('badRequest', 'Choose a folder to look in.');
      const id = this.nextJobId++;
      const job: MockJob = { spec, status: this.status(id, kind, spec, 1, { state: 'queued' }), step: 0, timer: undefined };
      this.jobBook.set(id, job);
      while (this.jobBook.size > 16) this.jobBook.delete(this.jobBook.keys().next().value as number);
      this.publish(job);
      this.change(job, { state: 'running', phase: null });
      this.schedule(job);
      return id;
    });
  }

  /** Stops a job as the agent would (the shell's `job_cancel`); false when it is unknown or over. */
  cancelJob(id: number): boolean {
    const job = this.jobBook.get(id);
    if (!job || !isActive(job.status)) return false;
    clearTimeout(job.timer);
    this.change(job, { state: 'cancelled', reason: 'user' });
    return true;
  }

  jobResult(id: number): Promise<JobOutput> {
    return this.reply('jobResult', () => {
      const j = this.jobBook.get(id);
      if (!j || j.status.state !== 'done') throw err('notFound', 'That job has no result: it is still running, was cancelled or failed, or is too old to be kept.');
      return this.outputOf(j.spec);
    });
  }

  private status(id: number, kind: JobKindSlug, spec: JobSpec, seq: number, state: JobStateFields): SuJobSnapshot {
    return {
      id,
      kind,
      title: JOB_TITLES[kind],
      subject: jobSubject(spec),
      seq,
      elapsedMs: 0,
      etaMs: null,
      cancellable: true,
      notCancellableReason: null,
      ...state,
    } as SuJobSnapshot;
  }

  private publish(job: MockJob): void {
    this.jobSink?.emit(job.status);
  }

  /** The next status of a job: the sequence number rises, the time moves, and the sink hears of it. */
  private change(job: MockJob, state: JobStateFields): void {
    job.status = { ...job.status, ...state, seq: job.status.seq + 1, elapsedMs: job.step * this.jobStepMs } as SuJobSnapshot;
    this.publish(job);
  }

  private schedule(job: MockJob): void {
    if (this.jobsStalled) return;
    job.timer = setTimeout(() => {
      job.step++;
      if (job.step < this.jobSteps) {
        this.change(job, { state: 'running', phase: mockPhase(job.status.kind as JobKindSlug, job.step, this.jobSteps - 1) });
        this.schedule(job);
        return;
      }
      const problem = this.problemWith(job.spec);
      if (problem) this.change(job, { state: 'failed', message: problem });
      else {
        if (job.status.kind === 'refresh-backup') this.backupCheckedAt = this.now();
        this.change(job, { state: 'done' });
      }
    }, this.jobStepMs);
  }

  /** What makes the work fail once it runs: the same messages the engine gives. */
  private problemWith(spec: JobSpec): string | null {
    const o = objectOf(spec);
    if ('findFiles' in o) {
      const folder = (o as { findFiles: { query: FinderQuery } }).findFiles.query.folder;
      return folder.startsWith('/') || /^[A-Za-z]:[\\/]/.test(folder) ? null : 'Give the full path of the folder.';
    }
    if ('fileHash' in o) {
      const source = (o as { fileHash: { source: HashSource } }).fileHash.source;
      const path = 'path' in source ? source.path : (this.find(source.process).exe ?? '');
      return path.startsWith('/') || /^[A-Za-z]:\\/.test(path) ? null : 'That is not a full path to a file.';
    }
    return null;
  }

  private outputOf(spec: JobSpec): JobOutput {
    const o = objectOf(spec);
    if ('findFiles' in o) {
      const query = (o as { findFiles: { query: FinderQuery } }).findFiles.query;
      return { finder: mockFinder({ ...query, minBytes: Math.max(query.minBytes, MB) }, this.now()) };
    }
    if ('rescanIssues' in o) {
      const query = (o as { rescanIssues: { query: IssuesQuery } }).rescanIssues.query;
      return { issues: mockIssuesOverview(query, this.now(), mockSources(this.now())) };
    }
    if ('fileHash' in o) {
      const source = (o as { fileHash: { source: HashSource } }).fileHash.source;
      const path = 'path' in source ? source.path : (this.find(source.process).exe ?? '');
      return { fileHash: { path, bytes: 48_231_424, sha256: mockHash(path) } };
    }
    if ('sleepBlockers' in o) return { sleep: mockSleep() };
    if ('bootAnalysis' in o) return { boot: mockBoot(this.now()) };
    if ('appUsage' in o) return { appUsage: mockUsage() };
    if ('softwareInventory' in o) return { software: mockSoftware() };
    return { backup: mockBackupReport(this.backupVariant, this.now(), this.backupCheckedAt) };
  }

  // ---------------------------------------------------------------------------------------------- plumbing

  /** Emits an alert as the agent would (dev tools and tests). */
  pushAlert(alert: Alert): void {
    this.alertLog = [alert, ...this.alertLog];
    for (const l of this.alertListeners) l(copy(alert));
  }

  /** Emits an agent status change (dev tools and tests). */
  pushAgentStatus(status: AgentStatus): void {
    for (const l of this.agentListeners) l(copy(status));
  }

  private find(target: ProcessRef): MockProcess {
    const p = this.processes.find((x) => x.pid === target.pid && x.started === target.started);
    if (!p) throw err('notFound', 'The process has exited.');
    return p;
  }

  /** Records an activity line and pushes it to the subscribers. */
  log(kind: ActivityEntry['kind'], title: string, detail: string): ActivityEntry {
    const entry: ActivityEntry = { id: `act-${this.nextId++}`, at: this.now(), kind, title, detail };
    this.activityLog = [entry, ...this.activityLog];
    queueMicrotask(() => this.emitActivity(entry));
    return entry;
  }

  private emitActivity(entry: ActivityEntry): void {
    for (const l of this.activityListeners) l(copy(entry));
  }

  /** Runs `body` after the simulated latency and resolves with a copy (callers can't mutate the mock's state). */
  /** Rewrites the Windows wording of the shared demo data for the other operating systems (mock only). */
  private localize<T>(value: T): T {
    if (this.profile.os === 'windows') return value;
    const os = this.profile.os === 'macos' ? 'macOS' : 'Linux';
    const fix = (text: string) =>
      text
        .replace(/\b([A-Za-z0-9_-]+)\.exe\b/gi, '$1')
        .replace(/Windows services/g, `${os} services`)
        .replace(/Windows started/g, this.profile.bootLabel)
        .replace(/Drive C:/g, this.profile.primaryDisk)
        .replace(/MsMpEng/g, this.profile.names.indexer)
        .replace(/Windows Search/g, this.profile.os === 'macos' ? 'Spotlight' : 'GNOME Tracker')
        .replace(/SearchIndexer/g, this.profile.names.indexer)
        .replace(/Recycle Bin/g, this.profile.os === 'macos' ? 'Trash' : 'Trash')
        .replace(/Microsoft Defender|Defender/g, this.profile.os === 'macos' ? 'XProtect' : 'The antivirus')
        .replace(/(?<!\\)\bWindows\b/g, os);
    const walk = (v: unknown): unknown => {
      if (typeof v === 'string') return fix(v);
      if (Array.isArray(v)) return v.map(walk);
      if (v && typeof v === 'object') return Object.fromEntries(Object.entries(v).map(([k, x]) => [k, walk(x)]));
      return v;
    };
    return walk(value) as T;
  }

  /** Requests held until the test releases them (`__mockHold('history')`, `__mockRelease('history')`; mock only, for e2e gates). */
  private readonly holds = new Map<string, { open: boolean; waiting: Array<() => void> }>();

  /** Holds the answers to `name` from now on, until `release(name)`. */
  hold(name: string): void {
    this.holds.set(name, { open: false, waiting: [] });
  }

  /** Lets the held answers to `name` through, and later ones too. */
  release(name: string): void {
    const h = this.holds.get(name);
    if (!h) return;
    h.open = true;
    for (const go of h.waiting.splice(0)) go();
  }

  private reply<T>(name: string, body: () => T, extraMs = 0): Promise<T> {
    return new Promise<T>((resolve, reject) => {
      const gate = this.holds.get(name);
      const run = () => {
        try {
          if (this.fail.has(name)) throw err('internal', `The mock was asked to fail “${name}”.`);
          resolve(this.localize(copy(body())));
        } catch (e) {
          reject(e);
        }
      };
      const delay = this.latencyMs === 0 ? 0 : this.latencyMs + extraMs;
      if (gate && !gate.open) gate.waiting.push(run);
      else if (delay === 0) queueMicrotask(run);
      else setTimeout(run, delay);
    });
  }
}

/** Where the mock pushes a job's status: the shell's in-memory jobs backend, standing in for the `paddock://jobs` event. */
export interface MockJobSink {
  emit(job: SuJobSnapshot): void;
}

interface MockJob {
  spec: JobSpec;
  status: SuJobSnapshot;
  step: number;
  timer: ReturnType<typeof setTimeout> | undefined;
}

type JobKindSlug = 'find-files' | 'rescan-issues' | 'refresh-backup' | 'file-hash' | 'sleep-blockers' | 'boot-analysis' | 'app-usage' | 'software-inventory';

/** The part of a snapshot that changes with its state (the shell's flat state tag). */
type JobStateFields = SuJobState;

const JOB_TITLES: Record<JobKindSlug, string> = {
  'find-files': 'Finding files',
  'rescan-issues': 'Reading system logs',
  'refresh-backup': 'Checking backups',
  'file-hash': 'Hashing a file',
  'sleep-blockers': 'Reading sleep blockers',
  'boot-analysis': 'Analysing boots',
  'app-usage': 'Reading app usage',
  'software-inventory': 'Listing installed software',
};

const SPEC_KIND: Record<string, JobKindSlug> = {
  findFiles: 'find-files',
  rescanIssues: 'rescan-issues',
  refreshBackup: 'refresh-backup',
  fileHash: 'file-hash',
  sleepBlockers: 'sleep-blockers',
  bootAnalysis: 'boot-analysis',
  appUsage: 'app-usage',
  softwareInventory: 'software-inventory',
};

/** The variant of a job spec as an object (a unit variant is a string on the wire). */
function objectOf(spec: JobSpec): Record<string, unknown> {
  return typeof spec === 'string' ? { [spec]: null } : spec;
}

function jobKind(spec: JobSpec): JobKindSlug {
  return SPEC_KIND[Object.keys(objectOf(spec))[0] as string] as JobKindSlug;
}

function jobSubject(spec: JobSpec): string | null {
  const o = objectOf(spec);
  if ('findFiles' in o) return (o as { findFiles: { query: FinderQuery } }).findFiles.query.folder;
  if ('rescanIssues' in o) return `last ${(o as { rescanIssues: { query: IssuesQuery } }).rescanIssues.query.days} days`;
  if ('fileHash' in o) {
    const source = (o as { fileHash: { source: HashSource } }).fileHash.source;
    return 'path' in source ? source.path : null;
  }
  return null;
}

function isActive(s: SuJobSnapshot): boolean {
  return s.state === 'queued' || s.state === 'running';
}

/** The phase of a mock job at step `step` of `last`: counts that only grow, each kind on its own scale. */
function mockPhase(kind: JobKindSlug, step: number, last: number): SuJobPhase {
  const part = step / Math.max(1, last);
  switch (kind) {
    case 'find-files':
      return { id: 'walk', label: 'Listing files', unit: 'items', done: step * 1_200, total: null };
    case 'rescan-issues':
      return { id: 'source-MacLog', label: 'Reading system log', unit: 'items', done: step * 640, total: null };
    case 'refresh-backup':
      return { id: 'tools', label: 'Checking backup tools', unit: 'items', done: Math.min(2, Math.floor(part * 3)), total: 3 };
    case 'file-hash':
      return { id: 'hash', label: 'Reading the file', unit: 'bytes', done: Math.floor(part * 48_231_424), total: 48_231_424 };
    default:
      return { id: 'read', label: JOB_TITLES[kind], unit: 'items', done: 0, total: null };
  }
}

function err(code: CommandError['code'], message: string): CommandError {
  return { code, message };
}

function publisher(p: MockProcess, fallback: string): string {
  const map: Record<string, string> = {
    chrome: 'Google LLC',
    vscode: 'Microsoft Corporation',
    teams: 'Microsoft Corporation',
    outlook: 'Microsoft Corporation',
    onedrive: 'Microsoft Corporation',
    slack: 'Slack Technologies, LLC',
    spotify: 'Spotify AB',
    docker: 'Oracle Corporation',
    node: 'OpenJS Foundation',
    terminal: 'Microsoft Corporation',
    pwsh: 'Microsoft Corporation',
    nvidia: 'NVIDIA Corporation',
    dyno: 'Dyno contributors',
    'dyno-agent': 'Dyno contributors',
  };
  return map[p.group] ?? fallback;
}
