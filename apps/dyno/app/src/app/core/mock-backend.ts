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
  AppInfo,
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
  Settings,
  DiskChildren,
  DiskChildrenQuery,
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
  HelperStatus,
  WeeklyReport,
  ReportFormat,
  SavedReport,
  BackupReport,
  BootReport,
  ChangeTimeline,
  ChangesQuery,
  FinderQuery,
  FinderResult,
  GpuReport,
  InventoryFormat,
  MaintenanceStatus,
  SavedInventory,
  SleepReport,
  SoftwareInventory,
  UsageReport,
  IssuesOverview,
  IssuesQuery,
  SavedIssues,
  UnusualItem,
  UnusualOverview,
  FileHashInfo,
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
}

/**
 * Reads the mock options from the page's query string: `?mock=` takes `windows`, `macos`, `linux` and/or `desktop`
 * (comma separated, e.g. `mock=linux,desktop`), `fail=health,whySlow` makes requests fail, `latency=0` removes the delay.
 */
export function mockOptionsFromUrl(search: string): MockOptions {
  const q = new URLSearchParams(search);
  const fail = q.get('fail');
  const latency = q.get('latency');
  const tokens = (q.get('mock') ?? '').split(',').map((t) => t.trim().toLowerCase());
  const os = (['windows', 'macos', 'linux'] as const).find((o) => tokens.includes(o));
  const cpu = tokens.includes('hybrid') ? 'hybrid' : tokens.includes('uniform') ? 'uniform' : undefined;
  return {
    machine: tokens.includes('desktop') ? 'desktop' : 'laptop',
    os: os ?? 'macos',
    ...(cpu ? { cpu } : {}),
    latencyMs: latency !== null && Number.isFinite(Number(latency)) ? Number(latency) : 150,
    fail: new Set(fail ? fail.split(',') : []),
    ...(q.get('backup') === 'none' ? { backup: 'none' as const } : {}),
  };
}

const copy = <T>(v: T): T => structuredClone(v);

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
 * build`. Live values are a deterministic function of the clock; the few requests that change anything (helper, settings,
 * snapshots, benchmarks) change the mock's state so those flows can be exercised. It mimics the contract; it is never the reference for behaviour.
 */
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
  private helperValue: HelperStatus = mockHelper('notInstalled');
  private startup: StartupEntry[] = demoStartup();
  private snapshotList: SnapshotInfo[];
  private readonly snapshotData = new Map<string, ProcessSample[]>();
  private nextId = 100;

  private readonly liveListeners = new Map<(u: LiveUpdate) => void, LiveDetail>();
  private readonly alertListeners = new Set<(a: Alert) => void>();
  private readonly activityListeners = new Set<(a: ActivityEntry) => void>();
  private readonly agentListeners = new Set<(s: AgentStatus) => void>();
  private readonly alarmListeners = new Set<(c: AlarmChange) => void>();
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
    this.counters = new MockCounters(this.profile.os);
    this.settingsValue = { ...this.settingsValue, counters: demoCounterSettings(this.profile.os) };
    this.latencyMs = options.latencyMs ?? 0;
    this.now = options.now ?? (() => Date.now());
    this.fail = options.fail ?? new Set();
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

  appInfo(): Promise<AppInfo> {
    return this.reply('appInfo', () => ({ version: '0.1.0', os: this.profile.os, updatesAvailable: false, updatesReason: 'Updates are not configured for this build (no signing key).' }));
  }

  /** The live sample at the current second. */
  liveUpdate(detail: LiveDetail): LiveUpdate {
    const t = Math.floor(this.now() / 1000);
    const samples = this.samplesAt(t);
    const groups = groupSamples(this.processes, samples);
    const system = this.system(t, samples);
    const byCpu = [...groups].sort((a, b) => b.cpu - a.cpu).slice(0, 8);
    const byMemory = [...groups].sort((a, b) => b.memory - a.memory).slice(0, 8);
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
    };
  }

  private samplesAt(t: number): ProcessSample[] {
    return this.processes.map((p) => sampleProcess(p, t, null, 1));
  }

  private system(t: number, samples: ProcessSample[]): SystemSample {
    const processCpu = samples.reduce((a, s) => a + s.cpu, 0);
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
    const memUsed = Math.min(memTotal - 1.2 * GB, samples.reduce((a, s) => a + s.memory, 0) + 2.9 * GB + swell);
    const diskRead = samples.reduce((a, s) => a + s.diskReadBps, 0);
    const diskWrite = samples.reduce((a, s) => a + s.diskWriteBps, 0);
    const building = samples.some((s) => s.name === this.profile.names.rustc && s.cpu > 1);
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
          ? { unavailable: 'Needs the Dyno helper (administrator) for system processes.' }
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
        startupSource: p.group === 'teams' ? 'Login item: Microsoft Teams' : p.group === 'docker' ? 'Login item: Docker Desktop' : null,
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
        .map((s) => ({ pid: s.pid, name: s.name, cpuBefore: xs.get(s.pid)!.cpu, cpuAfter: s.cpu, memoryBefore: xs.get(s.pid)!.memory, memoryAfter: s.memory }))
        .filter((c) => Math.abs(c.memoryAfter - c.memoryBefore) >= 10 * MB || Math.abs(c.cpuAfter - c.cpuBefore) >= 5)
        .sort((p, q) => Math.abs(q.memoryAfter - q.memoryBefore) - Math.abs(p.memoryAfter - p.memoryBefore));
      const sum = (l: ProcessSample[], f: (s: ProcessSample) => number) => l.reduce((acc, s) => acc + f(s), 0);
      return {
        before: first,
        after: second,
        added: y.filter((s) => !xs.has(s.pid)).map(summary),
        removed: x.filter((s) => !ys.has(s.pid)).map(summary),
        changed,
        memoryUsedDelta: Math.round(sum(y, (s) => s.memory) - sum(x, (s) => s.memory)),
        cpuDelta: round(sum(y, (s) => s.cpu) - sum(x, (s) => s.cpu), 1),
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
      const rustcCpu = rustc.reduce((a, s) => a + s.cpu, 0);
      const io = this.system(t, samples).diskIo;
      const queue = 'available' in io.queue ? io.queue.available : 0;
      if (rustc.length === 0 || rustcCpu < 5) {
        return { at: now, windowSecs: 55, headline: 'Nothing is slowing your PC down right now', causes: [{ kind: 'nothingWrong', title: 'All quiet', explanation: 'CPU, memory and disk all have room to spare.', confidence: 0.9, evidence: [], hints: [], involved: [] }] };
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
            hints: [{ text: 'Open the task manager', link: 'taskManager' }],
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
            explanation: 'Not slowing you down yet, but Windows needs free space for updates and the page file.',
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
        const disks = this.profile.disks(0);
        h.diskTrends.forEach((t, i) => (t.disk = disks[i]?.mount ?? t.disk));
      }
      return h;
    });
  }

  activity(limit: number): Promise<ActivityEntry[]> {
    return this.reply('activity', () => this.activityLog.slice(0, Math.max(0, limit)));
  }

  // ------------------------------------------------------------------------------------------------- rules

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

  diskPath(scan: number, node: number): Promise<string> {
    return this.reply('diskPath', () => this.disk.path(scan, node));
  }

  // ------------------------------------------------------------------------------------- health

  health(): Promise<HealthReport> {
    return this.reply('health', () => {
      const reasons = healthReasons();
      const score = 100 - reasons.reduce((a, r) => a + r.impact, 0);
      return { at: this.now(), score, grade: score >= 80 ? 'good' : score >= 60 ? 'fair' : 'poor', reasons };
    });
  }

  // ------------------------------------------------------------------------------------ the privileged helper

  helperStatus(): Promise<HelperStatus> {
    return this.reply('helperStatus', () => this.helperValue);
  }

  /** Opens the (pretend) permission prompt; the helper is ready a moment later, as the real one is once the prompt is answered. */
  installHelper(): Promise<HelperStatus> {
    return this.reply('installHelper', () => {
      if (this.latencyMs === 0) {
        this.helperValue = mockHelper('ready');
        return this.helperValue;
      }
      this.helperValue = mockHelper('installing');
      setTimeout(() => (this.helperValue = mockHelper('ready')), 800);
      return this.helperValue;
    });
  }

  uninstallHelper(): Promise<HelperStatus> {
    return this.reply('uninstallHelper', () => {
      this.helperValue = mockHelper('notInstalled');
      this.log('system', 'Removed the privileged helper', 'The helper is gone; Dyno keeps working without it.');
      return this.helperValue;
    });
  }

  private needHelper(): void {
    if (this.helperValue.state !== 'ready') {
      throw err('accessDenied', 'This change needs Dyno\'s helper, which is not installed. Install it under Settings > Helper (the system asks for your permission once); nothing else in Dyno needs it.');
    }
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

  fileHash(source: HashSource): Promise<FileHashInfo> {
    return this.reply('fileHash', () => {
      const path = 'path' in source ? source.path : (this.find(source.process).exe ?? '');
      if (!path.startsWith('/') && !/^[A-Za-z]:\\/.test(path)) throw err('notFound', 'That is not a full path to a file.');
      return { path, bytes: 48_231_424, sha256: mockHash(path) };
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
      mockRun('r3', this.start - 2 * 60 * MIN, 'Power mode: High performance (after)', 1.12, { group: 'Power mode: High performance', phase: 'after' }),
      mockRun('r2', this.start - 3 * 60 * MIN, 'Power mode: High performance (before)', 1, { group: 'Power mode: High performance', phase: 'before' }),
      mockRun('r1', this.start - 2 * DAY, 'Baseline', 0.99, { load: { cpuPercent: 31, memoryPercent: 61, onBattery: false } }),
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
            results: kinds.map((k) => mockKindResult(k, r.phase === 'after' ? 1.12 : 1, false)),
          });
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
          'Your PC averaged 38% CPU and 61% memory, peaking at 94% CPU.',
          'Docker’s backend crashed 6 times today: check its WSL integration.',
          'Sign-in takes 9 s longer than a month ago: 2 new startup apps.',
        ],
        scoreTrend: [66, 68, 71, 70, 72, 74].map((score, i) => ({ at: from + i * 28 * 60 * MIN, score })),
        disks: [
          { disk: this.profile.os === 'windows' ? 'C:' : '/', total: 953 * GB, free: 210 * GB, forecast: { fullInDays: 41 } },
          { disk: this.profile.os === 'windows' ? 'D:' : '/Volumes/Data', total: 2048 * GB, free: 1500 * GB, forecast: 'notFilling' },
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

  rescanIssues(query: IssuesQuery): Promise<IssuesOverview> {
    return this.reply('rescanIssues', () => mockIssuesOverview(query, this.now(), mockSources(this.now())));
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

  plugins(): Promise<PluginInfo[]> {
    return this.reply('plugins', () => demoPlugins());
  }

  trayOpenMain(): Promise<void> {
    return this.reply('trayOpenMain', () => undefined);
  }

  popoverResize(_height: number): Promise<void> {
    return Promise.resolve();
  }

  trayQuit(): Promise<void> {
    return this.reply('trayQuit', () => undefined);
  }

  // ----------------------------------------------------------------------------------------- extras, backup

  changes(query: ChangesQuery): Promise<ChangeTimeline> {
    return this.reply('changes', () => mockChanges(query, this.now()));
  }

  sleepBlockers(): Promise<SleepReport> {
    return this.reply('sleepBlockers', () => mockSleep());
  }

  bootAnalysis(): Promise<BootReport> {
    return this.reply('bootAnalysis', () => mockBoot(this.now()));
  }

  appUsage(): Promise<UsageReport> {
    return this.reply('appUsage', () => mockUsage());
  }

  gpuProcesses(): Promise<GpuReport> {
    return this.reply('gpuProcesses', () => mockGpu());
  }

  softwareInventory(): Promise<SoftwareInventory> {
    return this.reply('softwareInventory', () => mockSoftware());
  }

  saveInventoryReport(format: InventoryFormat): Promise<SavedInventory | null> {
    return this.reply('saveInventoryReport', () => {
      this.savedInventories.push(format);
      return { path: `/Users/sam/Documents/dyno-inventory.${format === 'markdown' ? 'md' : 'json'}`, bytes: 18_204 };
    });
  }

  pickFolder(): Promise<string | null> {
    return this.reply('pickFolder', () => {
      this.pickedFolders++;
      return '/Users/sam/Projects';
    });
  }

  findFiles(query: FinderQuery): Promise<FinderResult> {
    return this.reply('findFiles', () => {
      if (!query.folder.startsWith('/') && !/^[A-Za-z]:[\\/]/.test(query.folder)) throw err('badRequest', 'Choose a folder with a full path.');
      return mockFinder({ ...query, minBytes: Math.max(query.minBytes, MB) }, this.now());
    });
  }

  maintenance(): Promise<MaintenanceStatus> {
    return this.reply('maintenance', () => mockMaintenanceStatus(this.settingsValue.maintenance, this.now()));
  }

  backup(): Promise<BackupReport> {
    return this.reply('backup', () => mockBackupReport(this.backupVariant, this.now(), this.backupCheckedAt));
  }

  refreshBackup(): Promise<BackupReport> {
    return this.reply('refreshBackup', () => {
      this.backupCheckedAt = this.now();
      return mockBackupReport(this.backupVariant, this.now(), this.backupCheckedAt);
    }, 400);
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

  private log(kind: ActivityEntry['kind'], title: string, detail: string): ActivityEntry {
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

  private reply<T>(name: string, body: () => T, extraMs = 0): Promise<T> {
    return new Promise<T>((resolve, reject) => {
      const run = () => {
        try {
          if (this.fail.has(name)) throw err('internal', `The mock was asked to fail “${name}”.`);
          resolve(this.localize(copy(body())));
        } catch (e) {
          reject(e);
        }
      };
      const delay = this.latencyMs === 0 ? 0 : this.latencyMs + extraMs;
      if (delay === 0) queueMicrotask(run);
      else setTimeout(run, delay);
    });
  }
}

/** The helper as the mock reports it (the real wording comes from the engine). */
function mockHelper(state: HelperStatus['state']): HelperStatus {
  const text: Record<HelperStatus['state'], string> = {
    notInstalled: 'The helper is not installed. Dyno works without it; install it to read the few things that need administrator rights (for example programs\' network traffic on Windows).',
    installing: "Waiting for you to answer the system's permission prompt.",
    ready: 'The helper is installed and ready. It only reads and changes nothing on this computer.',
    outdated: 'The installed helper is older than this Dyno. Update the helper.',
    unreachable: 'The helper is installed but did not answer. Install it again from Settings to repair it.',
    unsupported: 'The helper cannot be installed on this computer.',
  };
  return {
    state,
    message: text[state],
    version: state === 'ready' ? '0.1.0' : null,
    prompt: 'The system asks you to confirm with your account password or a Windows permission prompt.',
    recent: [],
    error: null,
  };
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
    docker: 'Docker Inc.',
    node: 'OpenJS Foundation',
    terminal: 'Microsoft Corporation',
    pwsh: 'Microsoft Corporation',
    nvidia: 'NVIDIA Corporation',
    dyno: 'Dyno contributors',
    'dyno-agent': 'Dyno contributors',
  };
  return map[p.group] ?? fallback;
}
