/**
 * The UI ↔ app wire contract (camelCase JSON). Mirrors the Rust types in `crates/dyno-core` (serde, externally
 * tagged enums: a unit variant is a string, a data variant is `{ variant: data }`). Change both sides together.
 */
import type { Metric } from './generated/widgets';

/** Milliseconds since the Unix epoch. */
export type Timestamp = number;

/** A measurement the platform may not provide, with the reason when it doesn't. */
export type Reading<T> = { available: T } | { unavailable: string };

// ---------------------------------------------------------------------------------------------------------- metrics

/** What kind of core a logical core is on a hybrid CPU. */
export type CoreKind = 'performance' | 'efficiency' | 'unknown';

export interface CpuSample {
  total: number;
  perCore: number[];
  /** Kind of each logical core (same length as `perCore`), or empty when the OS exposes no topology / the CPU is uniform. */
  coreKinds: CoreKind[];
  /** Current frequency of each logical core in MHz (same length as `perCore`), or empty when unknown. */
  coreFrequencyMhz: number[];
  frequencyMhz: number;
  baseFrequencyMhz: number;
}

export type MemoryPressure = 'normal' | 'elevated' | 'critical';

export interface Commit {
  used: number;
  limit: number;
}

export interface MemorySample {
  total: number;
  used: number;
  available: number;
  commit: Reading<Commit>;
  swapUsed: number;
  swapTotal: number;
  pressure: Reading<MemoryPressure>;
}

export interface DiskSample {
  id: string;
  name: string;
  mount: string;
  total: number;
  free: number;
  removable: boolean;
}

export interface DiskIo {
  readBps: number;
  writeBps: number;
  queue: Reading<number>;
  busy: Reading<number>;
}

export interface NetworkSample {
  name: string;
  rxBps: number;
  txBps: number;
}

export interface GpuSample {
  name: string;
  utilization: number;
  memoryUsed: number;
  memoryTotal: number;
}

export type PowerSource = 'ac' | 'battery';

export interface BatterySample {
  charge: number;
  source: PowerSource;
  health: number;
  cycles: number | null;
  secondsToEmpty: number | null;
}

export interface Temperature {
  label: string;
  celsius: number;
  critical: number | null;
}

export interface Fan {
  label: string;
  rpm: number;
}

export interface SystemSample {
  at: Timestamp;
  cpu: CpuSample;
  memory: MemorySample;
  disks: DiskSample[];
  diskIo: DiskIo;
  network: NetworkSample[];
  gpu: Reading<GpuSample>;
  battery: Reading<BatterySample>;
  temperatures: Reading<Temperature[]>;
  fans: Reading<Fan[]>;
  uptimeSecs: number;
}

// -------------------------------------------------------------------------------------------------------- processes

export type RunState = 'running' | 'sleeping' | 'suspended' | 'other';

/** Why a per-process value was not read. */
export type UnavailableReason = 'needsAdmin' | 'notPermitted' | 'notSupported' | 'residentMemoryOnly';

/** A value that was read, or why it was not. A value that was not read is never `0`. */
export type Measured<T> = { value: T } | { unavailable: { reason: UnavailableReason } };

/** A sum over per-process values: only the readable ones are added up. */
export interface PartialSum<T> {
  sum: Measured<T>;
  partial: boolean;
}

/** Whose process it is, relative to the account Dyno runs as. */
export type Owner = 'own' | 'system' | 'other' | 'unknown';

/** Other users' processes folded into one row by the engine. */
export interface OtherUsers {
  count: number;
  cpu: Measured<number>;
  memory: Measured<number>;
  /** Some processes' CPU or memory could not be read, so the sums are lower than the real totals. */
  partial: boolean;
}

export interface ProcessSample {
  pid: number;
  parent: number | null;
  name: string;
  exe: string | null;
  user: Measured<string>;
  /** Percent of the whole machine, 0–100. */
  cpu: Measured<number>;
  memory: Measured<number>;
  diskReadBps: Measured<number>;
  diskWriteBps: Measured<number>;
  started: Measured<Timestamp>;
  owner: Owner;
  state: RunState;
}

/** A pid plus its start time: actions never hit a reused pid. */
export interface ProcessRef {
  pid: number;
  /** Absent when the OS did not give the start time; the pid alone identifies the process then. */
  started: Timestamp | null;
}

export type Priority = 'idle' | 'belowNormal' | 'normal' | 'aboveNormal' | 'high';

export interface ParentLink {
  pid: number;
  name: string;
}

export type ListOrReason<T> = { items: { items: T[]; truncated: boolean } } | { unavailable: string };

export interface Connection {
  protocol: string;
  local: string;
  remote: string;
  state: string;
}

export type SignatureInfo = { signed: string } | 'unsigned' | { invalid: string } | { unknown: string };

export interface ProcessDetails {
  process: ProcessSample;
  commandLine: string[];
  cwd: string | null;
  priority: Priority | null;
  affinity: number | null;
  efficiencyMode: boolean | null;
  threads: number | null;
  handles: number | null;
  parents: ParentLink[];
  signature: SignatureInfo;
  openFiles: ListOrReason<string>;
  connections: ListOrReason<Connection>;
  modules: ListOrReason<string>;
  startupSource: string | null;
}

/** What to do about a reading that could not be given (the engine's `NextStep`). */
export type NextStep = 'retry' | 'updateHelper' | 'updateApp' | 'showLog' | { openSettings: string } | { none: { why: string } };

/** Who read a reading (the engine's `ReadVia`): the helper, this computer with the person's rights, or nobody, with the reason. */
export type ReadVia = 'helper' | 'local' | { needsHelper: { reason: string; next: NextStep } } | { unavailable: { reason: string; next: NextStep | null } };

/** Where the parts of one live push came from. */
export interface LiveVia {
  processes: ReadVia;
  others: ReadVia;
  temperatures: ReadVia;
}

export interface Locker {
  process: ProcessRef;
  name: string;
  kind: string | null;
  /** Who read it. Always sent; `local` for a reading that never needed the helper. */
  via: ReadVia;
}

/** One application with all its processes. */
export interface AppGroup {
  id: string;
  name: string;
  exe: string | null;
  pids: number[];
  cpu: Measured<number>;
  memory: Measured<number>;
  diskBps: Measured<number>;
  /** Some member's value could not be read, so the sums are lower than the real totals. */
  partial: boolean;
}

export interface SnapshotInfo {
  id: string;
  at: Timestamp;
  label: string;
  processCount: number;
}

export interface ProcessSummary {
  pid: number;
  name: string;
  exe: string | null;
  cpu: Measured<number>;
  memory: Measured<number>;
}

export interface ProcessChange {
  pid: number;
  name: string;
  cpuBefore: number;
  cpuAfter: number;
  memoryBefore: number;
  memoryAfter: number;
}

export interface SnapshotDiff {
  before: SnapshotInfo;
  after: SnapshotInfo;
  added: ProcessSummary[];
  removed: ProcessSummary[];
  changed: ProcessChange[];
  memoryUsedDelta: number;
  cpuDelta: number;
}

// ------------------------------------------------------------------------------------------------------------ live

/** Dyno's own cost (shown in the UI; kept under ~1% CPU). */
export interface Overhead {
  agentCpu: number;
  agentMemory: number;
  samplingIntervalMs: number;
  processIntervalMs: number;
}

/** Pushed every tick on the `dyno://live` event. */
export interface LiveUpdate {
  system: SystemSample;
  /** The OS's pressure level where it reports one, else derived by the engine. */
  pressure: MemoryPressure;
  topCpu: AppGroup[];
  topMemory: AppGroup[];
  overhead: Overhead;
  /** Every process and group: only while a view subscribed to processes (the task manager). */
  processes: ProcessSample[] | null;
  groups: AppGroup[] | null;
  /** Other users' processes, folded into one row (`processes` and `groups` leave them out); null when there are none. */
  otherUsers: OtherUsers | null;
  /** Who read the processes, the "Other users" total and the temperatures. */
  via: LiveVia;
}

/** `tray` is the shell's own level with no window open (the menu bar icons); a window asks for `summary` or `processes`. */
export type LiveDetail = 'summary' | 'processes' | 'tray';

// --------------------------------------------------------------------------------------------------- why slow

export type CauseKind =
  | 'nothingWrong'
  | 'cpuHog'
  | 'memoryPressure'
  | 'diskQueue'
  | 'thermalThrottling'
  | 'osUpdate'
  | 'antivirusScan'
  | 'indexing'
  | 'lowDiskSpace'
  | 'gpuBusy';

export interface Evidence {
  label: string;
  value: string;
}

/** A page that shows more about a hint. */
export type HintLink = 'taskManager' | 'startup';

/** Something worth knowing about a cause, in words, with a page to look closer at where one helps (never an action). */
export interface CauseHint {
  text: string;
  link: HintLink | null;
}

export interface Cause {
  kind: CauseKind;
  title: string;
  explanation: string;
  /** 0–1. */
  confidence: number;
  evidence: Evidence[];
  hints: CauseHint[];
  /** The busiest apps behind the cause (empty when it is not an app). */
  involved: Involved[];
}

/** An app that is part of a cause, with its share of what was measured ("63% CPU", "1.2 GB"). */
export interface Involved {
  id: string;
  name: string;
  share: string;
}

export interface Diagnosis {
  at: Timestamp;
  /** How many seconds of samples the diagnosis looked at. */
  windowSecs: number;
  headline: string;
  causes: Cause[];
}

export type AnomalyKind = 'memoryGrowth' | 'periodicSpike' | 'restartLoop';

export interface Anomaly {
  id: string;
  kind: AnomalyKind;
  app: string;
  since: Timestamp;
  title: string;
  explanation: string;
  evidence: Evidence[];
}

// ------------------------------------------------------------------------------------------------------- history

export type HistoryRange = 'hour' | 'day' | 'week' | 'month';

export interface HistoryPoint {
  at: Timestamp;
  cpu: number;
  cpuMin: number;
  cpuMax: number;
  memory: number;
  memoryMin: number;
  memoryMax: number;
  diskReadBps: number;
  diskWriteBps: number;
  netRxBps: number;
  netTxBps: number;
  gpu: number | null;
  temperature: number | null;
}

export type TimelineEventKind = 'alert' | 'boot' | 'network';

export interface TimelineEvent {
  at: Timestamp;
  kind: TimelineEventKind;
  label: string;
}

export interface Consumer {
  app: string;
  cpuAvg: number;
  memoryAvg: number;
}

export type Forecast = { fullInDays: number } | 'notFilling' | 'notEnoughData';

export interface DiskTrendPoint {
  at: Timestamp;
  free: number;
}

export interface DiskTrend {
  disk: string;
  total: number;
  points: DiskTrendPoint[];
  forecast: Forecast;
}

export interface BootRecord {
  at: Timestamp;
  /** Boot to the login session starting, seconds. */
  bootSecs: number | null;
  /** Login to the desktop settling (CPU calm), seconds. */
  readySecs: number | null;
}

export interface DayCount {
  day: Timestamp;
  crashes: number;
  restarts: number;
}

/** Minimum, average and maximum load of one logical core over a bucket, percent. */
export interface CoreStat {
  min: number;
  avg: number;
  max: number;
}

/** Per-core load for one bucket (every logical core, in the order of `CpuSample.perCore`). */
export interface CoreBucket {
  at: Timestamp;
  cores: CoreStat[];
}

export interface History {
  range: HistoryRange;
  from: Timestamp;
  to: Timestamp;
  resolutionMs: number;
  points: HistoryPoint[];
  events: TimelineEvent[];
  topConsumers: Consumer[];
  diskTrends: DiskTrend[];
  boots: BootRecord[];
  crashes: DayCount[];
  /** Per-core load per bucket (same tier and resolution as `points`); empty when the range has none. */
  corePoints: CoreBucket[];
}

// --------------------------------------------------------------------------------------------------------- rules

export type ActivityKind = 'alert' | 'system';

export interface ActivityEntry {
  id: string;
  at: Timestamp;
  kind: ActivityKind;
  title: string;
  detail: string;
}

// ----------------------------------------------------------------------------------------------------- disk map

/** `Settings.disk`: every key optional. */
export interface DiskSettings {
  /** The most nodes one scan keeps (10,000 to 5,000,000). */
  scanFolders?: number;
  /** Files of at least this many MiB get a node of their own (1 to 1,024). */
  notableFileMib?: number;
  /** Threads that read folders at once, for the disk map and the file finder: Auto (by the kind of volume) or 1 to 16. */
  readThreads?: Threads;
  /** The file finder stops after this many files and folders (1,000 to 5,000,000). */
  finderFiles?: number;
  /** The file finder does not enter folders deeper than this (1 to 256). */
  finderDepth?: number;
  /** The file finder stops comparing contents after reading this many GiB (1 to 4,096). */
  finderHashGib?: number;
  /** The disk maps kept for the next time (ADR 0029). */
  savedMaps?: SavedMapsSettings;
}

/** When a saved map is refreshed by itself: never (the default), or at the first look at the page when it is older than this. */
export type RefreshOnOpen = 'off' | 'olderThanDay' | 'olderThanWeek';

/** `Settings.disk.savedMaps`: every key optional. */
export interface SavedMapsSettings {
  /** Whether finished scans are kept for the next time; switching it off forgets every saved map. */
  enabled?: boolean;
  /** The most all saved maps may take on disk, in MB (64 to 8,192). */
  maxMb?: number;
  /** Whether a saved map is refreshed at the first look at the page. */
  refreshOnOpen?: RefreshOnOpen;
}

/** A thread count setting: the engine decides (`'auto'`) or exactly this many (the engine holds it to the setting's own range). */
export type Threads = 'auto' | { fixed: number };

/** `Settings.jobs`: every key optional. */
export interface JobSettings {
  /** How many jobs run at once: Auto (4) or 1 to 8. Read when Dyno starts. */
  workers?: Threads;
}

/** The protocol of a network volume. */
export type NetworkProtocol = 'smb' | 'nfs' | 'afp' | 'webDav' | 'sshfs' | 'other';

/** Where a volume lives (externally tagged: a unit variant is a string). */
export type VolumeLocation = 'internal' | { external: { bus: string | null } } | { network: { protocol: NetworkProtocol; host: string } };

/** A volume the disk map can scan. */
export interface Volume {
  /** Stable id: the mount point. */
  id: string;
  name: string;
  mount: string;
  /** File system ("apfs", "ntfs", "smbfs"). */
  fs: string;
  location: VolumeLocation;
  /** Bytes; `null` when the volume cannot say. */
  capacity: number | null;
  free: number | null;
  /** A spinning disk; `null` when unknown. */
  rotational: boolean | null;
  readOnly: boolean;
  /** False when a network volume did not answer ("Not answering"). */
  reachable: boolean;
  /** Whether used space is a fixed scale for a scan; false for a network share (scanned like a folder). */
  wholeVolume: boolean;
}

/** What a scan covers. */
export type DiskTarget = { volume: { id: string } } | { folder: { path: string } };

export type ScanPhase = 'readingVolumeFacts' | 'readingFolders';

export type ScanFailure = { volumeUnreachable: { detail: string } } | { cannotStart: { reason: string } } | { internal: { detail: string } };

export type ScanState = 'running' | 'done' | 'cancelled' | { failed: { failure: ScanFailure } };

/** Live numbers of a scan; every one only grows. */
export interface ScanProgress {
  bytes: number;
  items: number;
  folders: number;
  /** The bytes expected (a whole volume's used space), or `null`: the total is unknown and the count just grows. */
  totalBytes: number | null;
  /** Folders the OS refused to open (counted, not fatal). */
  unreadable: number;
}

/** One scan's status: the reply to starting it and the pushed event while it runs (keep the highest `seq`). */
export interface DiskScanStatus {
  scan: number;
  seq: number;
  name: string;
  target: DiskTarget;
  volume: Volume | null;
  state: ScanState;
  phase: ScanPhase;
  progress: ScanProgress;
  /** A limit left folders out. */
  truncated: boolean;
  /** The first folders that could not be read. */
  unreadableFirst: string[];
  /** Folders were refused and this Mac has not given Dyno Full Disk Access, the likely reason (macOS only). */
  needsFullDiskAccess: boolean;
  /** Files and folders counted per second while running; `null` until it can be known (never a made-up number). */
  itemsPerSecond: number | null;
  /** Seconds left in the first pass over a whole volume; `null` when it cannot be known. */
  etaSeconds: number | null;
  /** Used space the scan did not count (used minus counted), for a whole-volume scan that is done; `null` otherwise. */
  hiddenBytes: number | null;
  /** When the latest pass ended; `null` while the first pass runs. */
  scannedAt: Timestamp | null;
  /** Scanned in this run, or read back from the saved-maps cache (ADR 0029), where it may be old (its age is `scannedAt`). */
  origin: ScanOrigin;
  /** For a saved whole-volume map: the net change in used space since the scan (used now minus used then); `null` otherwise. */
  changedBytes: number | null;
  /** The kept scan this one replaces (a Rescan): that map stays on screen until this pass is done. */
  replaces: number | null;
  /** Why the finished scan was not saved; `null` when it was, or saving is off. */
  notSaved: NotSaved | null;
}

/** Where a scan's tree came from. */
export type ScanOrigin = 'live' | { saved: { savedAt: Timestamp } };

/** Why a finished scan was not kept in the saved-maps cache. */
export type NotSaved = 'tooBig' | 'noSpace' | 'failed';

export type NodeKind = 'folder' | 'file' | 'mountPoint' | 'unreadable';

/** Why one folder could not be read: macOS privacy (Full Disk Access), missing rights, protected by macOS in another way, or another error. */
export type UnreadableReason = 'needsFullDiskAccess' | 'needsAdmin' | 'protected' | 'notAnswering' | 'other';

/** A node of a scan. */
export interface DiskNode {
  id: number;
  name: string;
  kind: NodeKind;
  /** Bytes on the volume, children included. */
  bytes: number;
  /** Bytes of content, children included (what the files add up to when read). */
  logicalBytes: number;
  /** Files and folders inside, recursively. */
  items: number;
  newestChange: Timestamp;
  /** Everything inside has been read. */
  complete: boolean;
  hasChildren: boolean;
  /** Why an unreadable folder could not be read (the engine decides, by the OS); null for every other node. */
  unreadableReason: UnreadableReason | null;
}

/** A sum of folded things. */
export interface Folded {
  count: number;
  /** Bytes on the volume. */
  bytes: number;
  /** Bytes of content. */
  logicalBytes: number;
  items: number;
}

/** One page of one level: the children of a folder, biggest first. children + other + loose = the folder's bytes. */
export interface DiskChildren {
  node: DiskNode;
  children: DiskNode[];
  /** Children below the minimum share, folded into one bucket (all pages). */
  other: Folded;
  /** Small files directly inside the folder that have no node of their own. */
  loose: Folded;
  /** How many children are shown across all pages. */
  shown: number;
  offset: number;
  /** The offset of the next page, `null` on the last. */
  nextOffset: number | null;
}

/** A folder or file with what is shown inside it, nested. */
export interface RingNode {
  node: DiskNode;
  /** Shown children, biggest first. */
  children: RingNode[];
  /** Children not shown: below the minimum share, or past the node cap. */
  other: Folded;
  /** Small files directly inside that have no node of their own. */
  loose: Folded;
}

/** Several levels of a scan under a folder, in one answer. */
export interface DiskRings {
  root: RingNode;
  /** Nodes in the answer, the centre included. */
  count: number;
  /** The node cap left out children that were big enough to show. */
  capped: boolean;
}

/** What to read: `rings` 1 to 6, `minShare` 0 to 1 of the centre, `maxNodes` 1 to 2000. */
export interface DiskRingsQuery {
  scan: number;
  /** The centre; `null` for the scan's root. */
  node: number | null;
  rings: number;
  minShare: number;
  maxNodes: number;
}

/** Which page of which level. */
export interface DiskChildrenQuery {
  scan: number;
  /** The folder; `null` for the scan's root. */
  node: number | null;
  offset: number;
  /** 1 to 256. */
  limit: number;
  /** Children below this share (0 to 1) of the folder are folded into `other`; 0 shows all. */
  minFraction: number;
}

// ------------------------------------------------------------------------------------------- health

export type Grade = 'good' | 'fair' | 'poor';
export type Severity = 'info' | 'warning' | 'critical';

export interface HealthReason {
  id: string;
  title: string;
  detail: string;
  /** Points this costs the score. */
  impact: number;
  severity: Severity;
  /** For an active alarm: when its condition first appeared; null for performance reasons. */
  since: Timestamp | null;
}

export interface HealthReport {
  at: Timestamp;
  /** The score after active alarms cap it. */
  score: number;
  grade: Grade;
  /** What performance alone scores, before alarms cap it. */
  performanceScore: number;
  /** Set when an active alarm of this severity lowered the score (warning caps at Fair, critical at Poor). */
  cappedBy: Severity | null;
  reasons: HealthReason[];
}

export type StartupLocation =
  | 'userRunKey'
  | 'machineRunKey'
  | 'userStartupFolder'
  | 'commonStartupFolder'
  | 'scheduledTask'
  | 'userLaunchAgent'
  | 'systemLaunchAgent'
  | 'loginItem';

export type StartupState = 'enabled' | 'disabled';

export type ImpactRating = 'notMeasured' | 'low' | 'medium' | 'high';

export interface StartupImpact {
  cpuSecs: number;
  diskBytes: number;
  rating: ImpactRating;
}

export interface StartupEntry {
  id: string;
  name: string;
  command: string;
  exe: string | null;
  location: StartupLocation;
  state: StartupState;
  publisher: string | null;
  impact: StartupImpact;
}

// -------------------------------------------------------------------------------------------- alerts and reports

export type AlertKind = 'lowDisk' | 'overheating' | 'memoryPressure' | 'crashLoop' | 'longHighCpu';

export interface Alert {
  id: string;
  at: Timestamp;
  kind: AlertKind;
  severity: Severity;
  title: string;
  detail: string;
}

export interface WeeklyReport {
  from: Timestamp;
  to: Timestamp;
  /** The health score at the end of the week; null when none was recorded. */
  score: number | null;
  scoreChange: number;
  avgCpu: number;
  peakCpu: number;
  avgMemory: number;
  topApps: Consumer[];
  crashes: number;
  alerts: number;
  highlights: string[];
  scoreTrend: ScorePoint[];
  disks: ReportDisk[];
  /** The week's alerts, newest first (at most 20). */
  issues: ReportIssue[];
  /** Hardware sentences: time throttled, disks and batteries that need attention (empty when there is nothing to say). */
  hardware: string[];
  /** True when the week holds nothing: a valid report that says so. */
  nothingToReport: boolean;
}

export interface ScorePoint {
  at: Timestamp;
  score: number;
}

export interface ReportDisk {
  disk: string;
  total: number;
  free: number;
  forecast: Forecast;
}

export interface ReportIssue {
  at: Timestamp;
  severity: Severity;
  title: string;
  detail: string;
}

export type ReportFormat = 'markdown' | 'html';

export interface SavedReport {
  path: string;
  bytes: number;
}

// ------------------------------------------------------------------------------------------------------ settings

export interface Settings {
  version: number;
  launchAtLogin: boolean;
  /** `helperSweepMs` is how often the helper reads every account's processes; `null` is Auto (5 s, never faster than the process table). */
  sampling: { intervalMs: number; processIntervalMs: number; helperSweepMs: number | null };
  retention: { rawHours: number; minuteDays: number; quarterHourDays: number; hourDays: number };
  alerts: {
    enabled: boolean;
    lowDiskPercent: number;
    overheatCelsius: number;
    longHighCpuMinutes: number;
    trayAlertPercent: number;
  };
  webApi: { enabled: boolean; port: number; lan: boolean };
  // The dashboard, menu-bar items and popup layouts travel in the settings JSON too, but a saved `Settings` never replaces
  // them: they are read and edited through `widgetLayout` and `editWidgetLayout` (family ADR 0035).
  /** "Look up IP" templates (the engine fills the default when absent). */
  ipLookup?: IpLookupSettings;
  /** Alarm behaviour: quiet hours, chatter detection, retention (the engine fills the default when absent). */
  alarms?: AlarmConfig;
  /** Keep the last ten minutes at one-second resolution for captures. */
  flightRecorder?: boolean;
  /** Rules that raise an alarm when a metric stays past a limit (all off until added and enabled). */
  customAlerts?: CustomAlertRule[];
  /** Alarms for new exposed listeners and new remote hosts (both off by default). */
  networkAlerts?: NetworkAlerts;
  /** Desktop notifications for new alarms (quiet hours and muted rules still apply). */
  notifications?: boolean;
  /** Pinned performance counters and saved counter sets (empty until the user adds some). */
  counters?: CounterSettings;
  /** "Is this normal?" baselines: learning period and alarms (the engine fills the default when absent). */
  baselines?: BaselineSettings;
  /** Which hardware findings may raise an alarm (the engine fills the default when absent). */
  hardwareAlarms?: HardwareAlarms;
  /** New and unusual programs: the baseline and its learning period (the engine fills the default when absent). */
  unusual?: UnusualSettings;
  /** The disk map's limits (the engine fills the defaults when absent). */
  disk?: DiskSettings;
  /** Long operations as jobs: how many run at once (the engine fills the default when absent). */
  jobs?: JobSettings;
  /** Maintenance windows: when Dyno's own heavier background work may run. */
  maintenance: MaintenanceSettings;
  /** Backup health checks. */
  backup: BackupSettings;
  /** Power: keep awake (ADR 0032). */
  power: PowerSettings;
}

export interface PluginInfo {
  id: string;
  name: string;
  version: string;
  state: 'running' | 'stopped' | 'failed' | 'quarantined';
  error: string | null;
  contributes: string[];
}

export interface AppInfo {
  version: string;
  os: 'windows' | 'macos' | 'linux';
  updatesAvailable: boolean;
  updatesReason: string | null;
}

export type AgentState = 'starting' | 'running' | 'restarting' | 'failed';

export interface AgentStatus {
  state: AgentState;
  message: string | null;
}

// --------------------------------------------------------------------------------------------------------- errors

export type ErrorCode =
  | 'agentUnavailable'
  | 'notFound'
  | 'accessDenied'
  | 'unsupported'
  | 'badRequest'
  | 'timeout'
  | 'busy'
  | 'needsConsent'
  | 'internal'
  | 'stale';

/** What every failed backend call rejects with. */
export interface CommandError {
  code: ErrorCode;
  message: string;
}

// ===================================================================================================================
// BEGIN rust-agent additions (crates/dyno-core: inventory, widgets, alarms, recorder, diagnostics). Mirrors the Rust DTOs.
// ===================================================================================================================

// ------------------------------------------------------------------------------------------------------- inventory

export type MemoryKind = 'ddr2' | 'ddr3' | 'ddr4' | 'ddr5' | 'lpddr3' | 'lpddr4' | 'lpddr5' | 'other' | 'unknown';
export type MemoryFormFactor = 'dimm' | 'soDimm' | 'soldered' | 'other' | 'unknown';

export interface MemoryModule {
  slot: string;
  bank: string | null;
  capacityBytes: number;
  speedMts: number | null;
  configuredSpeedMts: number | null;
  kind: MemoryKind;
  formFactor: MemoryFormFactor;
  manufacturer: string | null;
  partNumber: string | null;
}

export interface MemoryArray {
  /** 0 when unknown. */
  slots: number;
  maxCapacityBytes: number | null;
  modules: MemoryModule[];
}

export interface InventoryUnavailable {
  reason: string;
}

/** Externally tagged, like Rust's serde default. */
export type MemoryLayout =
  | { slots: MemoryArray }
  | { unified: { total: number; kind: MemoryKind; manufacturer: string | null; upgradeable: boolean } }
  | { totalOnly: { total: number; reason: InventoryUnavailable } };

export interface CpuInfo {
  name: string;
  physicalCores: number;
  logicalCores: number;
  frequencyMhz: number;
}

export interface DiskInfo {
  name: string;
  totalBytes: number;
  removable: boolean;
}

export type HintKind =
  | 'mismatchedSpeed'
  | 'belowRated'
  | 'mismatchedSize'
  | 'singleChannel'
  | 'multiChannel'
  | 'freeSlots'
  | 'maxCapacity'
  | 'notUpgradeable'
  | 'unknown';

export interface Hint {
  kind: HintKind;
  message: string;
  slot: string | null;
}

/** Reply of `inventory` (the shell command returns it as is). */
export interface SystemInventory {
  os: string;
  hostName: string | null;
  cpu: CpuInfo;
  memory: MemoryLayout;
  disks: DiskInfo[];
  hints: Hint[];
  /** Who read the memory layout. */
  memoryVia: ReadVia;
}

// ---------------------------------------------------------------------------------------------------------- alarms
// ISA-18.2 style alarms. Requests (agent protocol, camelCase): `alarms {filter}` -> Alarm[], `alarmStats` -> AlarmStats,
// `acknowledgeAlarms {ids}`, `shelveAlarm {id, until}`, `unshelveAlarm {id}`, `muteRule {rule, muted}`,
// `noteAlarm {id, text}` (each answers the changed Alarm[]), `alarmHistory {alarmId, limit}` -> AlarmChange[].
// Pushed message: `alarm` carrying an AlarmChange. `Settings.alarms` is an AlarmConfig.

export type AlarmState = 'active' | 'acknowledged' | 'cleared' | 'clearedUnacknowledged';
export type ShelveFor = 'oneHour' | 'untilTomorrow' | 'untilReboot' | { custom: Timestamp };
export type Shelf = { until: Timestamp } | 'untilReboot';

export interface AlarmNote {
  at: Timestamp;
  text: string;
}

export interface Alarm {
  id: number;
  /** The source rule, e.g. "alert:lowDisk" (what muting and statistics group by). */
  rule: string;
  /** The condition's identity; one open alarm per key. */
  key: string;
  severity: Severity;
  state: AlarmState;
  title: string;
  detail: string;
  /** Open the chart around this time. */
  firstSeen: Timestamp;
  lastSeen: Timestamp;
  repeatCount: number;
  currentValue: number | null;
  /** The metric to chart (null when none). */
  metric: Metric | null;
  notes: AlarmNote[];
  acknowledgedAt: Timestamp | null;
  clearedAt: Timestamp | null;
  shelved: Shelf | null;
  muted: boolean;
  chattering: boolean;
  expiresAt: Timestamp | null;
}

export interface AlarmFilter {
  /** Empty means every open alarm. */
  states: AlarmState[];
  minSeverity: Severity | null;
  rule: string | null;
  includeShelved: boolean;
  /** 1..500; 0 means 100. */
  limit: number;
}

export type AlarmChangeKind =
  | 'raised'
  | 'repeated'
  | 'returned'
  | 'acknowledged'
  | 'shelved'
  | 'unshelved'
  | 'muted'
  | 'noted'
  | 'chattering'
  | 'pruned';

export interface AlarmChange {
  kind: AlarmChangeKind;
  alarm: Alarm;
  /** Show a desktop notification. */
  notify: boolean;
  at: Timestamp;
}

export interface AlarmQuietHours {
  enabled: boolean;
  /** Minutes after local midnight, 0..1439; an end before the start wraps past midnight. */
  startMinute: number;
  endMinute: number;
  criticalBypass: boolean;
}

export interface AlarmConfig {
  quiet: AlarmQuietHours;
  /** 3..50 */
  chatterTransitions: number;
  /** 1..240 */
  chatterWindowMinutes: number;
  /** 50..5000 */
  maxAlarms: number;
  /** 1..90 */
  keepClearedDays: number;
}

export interface AlarmSuggestion {
  rule: string;
  kind: 'tune' | 'retire';
  message: string;
}

export interface AlarmStats {
  open: number;
  unacknowledged: number;
  chattering: string[];
  noisiest: { rule: string; occurrences: number }[];
  meanTimeToAcknowledgeMs: number | null;
  medianTimeToAcknowledgeMs: number | null;
  suggestions: AlarmSuggestion[];
}

// ------------------------------------------------------------------------------------------------------- recorder
// Agent requests: `capture` -> RecorderSnapshotInfo, `recorderSnapshots` -> RecorderSnapshotInfo[],
// `recorderSnapshot {id}` -> RecorderSnapshot. `Settings.flightRecorder: boolean` (default true).

export type CaptureReason = 'manual' | { alarm: { alarmId: number; title: string } };

export interface RecorderProcess {
  name: string;
  /** Percent of the machine. */
  cpu: number;
  memory: number;
}

export interface RecorderSample {
  at: Timestamp;
  cpuTotal: number;
  cpuCores: number[];
  memoryUsed: number;
  memoryTotal: number;
  diskReadBps: number;
  diskWriteBps: number;
  netRxBps: number;
  netTxBps: number;
  top: RecorderProcess[];
}

export interface RecorderSnapshotInfo {
  id: number;
  at: Timestamp;
  reason: CaptureReason;
  bytes: number;
  samples: number;
}

export interface RecorderSnapshot {
  version: number;
  at: Timestamp;
  reason: CaptureReason;
  /** About one per second, oldest first (thinned if the capture was large). */
  samples: RecorderSample[];
}

// ------------------------------------------------------------------------------------------------------ diagnostics
// Agent requests: `diagnosticsPreview` -> BundlePreview (nothing is written), `saveDiagnostics {path}` -> SavedDiagnostics.
// The path comes from the user's save dialog: absolute, ending in `.zip`, in an existing folder (the engine re-checks).

export interface RedactionCounts {
  homePaths: number;
  profilePaths: number;
  machineNames: number;
  userNames: number;
  emails: number;
  ipAddresses: number;
}

export interface BundleFileInfo {
  path: string;
  description: string;
  bytes: number;
}

export interface BundlePreview {
  /** Every file with its size, the manifest included. */
  files: BundleFileInfo[];
  totalBytes: number;
  redactions: RedactionCounts;
}

export interface SavedDiagnostics {
  path: string;
  bytes: number;
  files: number;
}

// ------------------------------------------------------------------------------------------------------ benchmarks
// Mirrors `dyno_core::bench`. Runs start only on a click; the engine answers `BenchOverview` and the UI polls it
// while `running` is set.

export type BenchKind = 'cpu' | 'memory' | 'disk' | 'gpu';
export type BenchPhase = 'before' | 'after';

export interface BenchMetric {
  id: string;
  label: string;
  value: number;
  unit: string;
  higherIsBetter: boolean;
  /** Every repetition, in the order measured; `value` is their median. */
  samples: number[];
  min: number;
  max: number;
  /** Coefficient of variation of the repetitions (null where the mean is zero). */
  cv: number | null;
  /** True when the repetitions varied by more than 10 %. */
  unsteady: boolean;
}

export type BenchStatus =
  | { state: 'done'; metrics: BenchMetric[] }
  | { state: 'unavailable'; reason: string }
  | { state: 'stopped'; reason: string }
  | { state: 'failed'; reason: string };

export interface BenchKindResult {
  kind: BenchKind;
  status: BenchStatus;
  millis: number;
  /** The method version it was measured with; 1 is Dyno 1.0's. */
  method: number;
  /** The device measured when a kind runs once per device (the drive's name for the disk test); absent otherwise. */
  device?: string | null;
  /** The drive's top is not writable: offer to choose a folder on it and run again. */
  needsFolder?: boolean;
  /** The sustained CPU run's trace, one point a second. */
  trace?: BenchTracePoint[];
  /** How it was measured, in words (file system, bus, whether the cache was bypassed, the vector width). */
  details?: string[];
}

export interface LoadAtRun {
  cpuPercent: number;
  memoryPercent: number;
  onBattery: boolean | null;
}

export interface BenchRun {
  id: string;
  startedAt: number;
  label: string;
  group: string | null;
  phase: BenchPhase | null;
  load: LoadAtRun;
  machine: string;
  results: BenchKindResult[];
  /** Measured the 1.0 way: kept, shown as "measured the old way", never compared with a current run. */
  oldWay: boolean;
  /** Why this run's numbers are low confidence, when they are. */
  limited: string | null;
}

/** Where the measuring program is, as it reports it. `rep` 0 is the warm-up; `total` is null while unknown. */
export interface BenchPhaseProgress {
  phase: string;
  label: string;
  rep: number;
  of: number;
  count: number;
  total: number | null;
}

/** How alive a run looks: nothing ends it but the person's Cancel (or a named safety stop). */
export type BenchLiveness = 'running' | 'stillWorking' | 'notResponding';

export interface BenchProgress {
  runId: string;
  startedAt: number;
  kinds: BenchKind[];
  current: BenchKind | null;
  done: BenchKind[];
  /** The phase being measured now (null between phases and kinds). */
  phase: BenchPhaseProgress | null;
  liveness: BenchLiveness;
  /** Set when the run cannot have full-confidence conditions (Linux without a user manager). */
  limited: string | null;
  /** The sustained run's points so far (the page draws them live); empty otherwise. */
  trace?: BenchTracePoint[];
}

/** One second of a sustained run. */
export interface BenchTracePoint {
  secs: number;
  /** Work done that second, in the metric's unit (GFLOPS). */
  throughput: number;
  freqMhz?: number | null;
  tempC?: number | null;
}

export interface BenchKindEstimate {
  kind: BenchKind;
  seconds: number;
}

export interface BenchDiskEstimate {
  mib: number;
  /** The most bytes the disk test writes to one drive (the engine's `disk_write_budget`). */
  bytesWritten: number;
  seconds: number;
}

/** What the picker shows before a run; the page adds these up for what is chosen. */
export interface BenchEstimates {
  kinds: BenchKindEstimate[];
  disk: BenchDiskEstimate[];
  sustainedMinutes: number[];
  noSensorMaxMinutes: number;
  sustainedWriteMaxGib: number;
  quickMib: number;
  /** The sustained disk write at each offered amount, per drive. */
  sustainedWrite: { gib: number; seconds: number }[];
}

export interface BenchOverview {
  running: BenchProgress | null;
  runs: BenchRun[];
  /** False while the engine keeps the disk test off (ADR 0030 D10); the page shows it as unavailable. */
  diskTestEnabled: boolean;
  estimates?: BenchEstimates;
}

/** A folder chosen for a drive whose top is not writable. */
export interface BenchDiskFolder {
  volume: string;
  folder: string;
}

export type BenchVerdict = 'better' | 'worse' | 'same';

export interface MetricDelta {
  kind: BenchKind;
  id: string;
  label: string;
  unit: string;
  higherIsBetter: boolean;
  before: number;
  after: number;
  changePercent: number;
  /** How widely each run's repetitions varied (max minus min). */
  beforeSpread: number;
  afterSpread: number;
  unsteady: boolean;
  verdict: BenchVerdict;
}

export interface BenchComparison {
  before: string;
  after: string;
  deltas: MetricDelta[];
  warnings: string[];
}

/** A graphics adapter, as the GPU runner lists it. */
export interface GpuAdapter {
  index: number;
  name: string;
  kind: 'discrete' | 'integrated' | 'virtual' | 'cpu' | 'other';
  backend: string;
  driver: string;
}

/** What a run is going to be, for the engine's estimate. */
export interface BenchEstimateRequest {
  kinds: BenchKind[];
  diskMib: number | null;
  /** How many drives are ticked. */
  drives: number;
  sustainedMinutes: number | null;
  sustainedWriteGib: number | null;
}

/** About how long it takes and the most it writes to each drive. */
export interface BenchRunEstimate {
  seconds: number;
  bytesPerDrive: number;
}

export type BenchExportFormat = 'json' | 'csv';

export interface StartBenchmark {
  kinds: BenchKind[];
  label: string;
  group?: string | null;
  phase?: BenchPhase | null;
  diskMib?: number | null;
  /** The person agreed to run on battery, for this run only. */
  allowBattery?: boolean;
  /** The volumes the disk test measures, by id (mount point); empty or absent means the system drive. */
  diskVolumes?: string[];
  /** Network drives may be tested in this run; their results are marked network. */
  allowNetworkDrives?: boolean;
  /** Folders chosen for drives whose top is not writable. */
  diskFolders?: BenchDiskFolder[];
  /** The graphics adapter to measure, by index in the adapter list; absent is the default (the discrete one). */
  gpuAdapter?: number | null;
  /** Run the CPU kind as a sustained run of this many minutes (2, 5, 10 or 20). */
  sustainedMinutes?: number | null;
  /** Also run the sustained disk write, up to this many GiB per drive; the person confirmed the amount. */
  sustainedWriteGib?: number | null;
}

// -------------------------------------------------------------------------------------------------------- network
// Mirrors `dyno_core::network` and `dyno_core::ip_lookup`. Observe-only: Dyno lists sockets, it never closes one.

export type NetProto = 'tcp' | 'udp';
export type SocketState = 'listen' | 'synSent' | 'synReceived' | 'established' | 'finWait' | 'closeWait' | 'closing' | 'lastAck' | 'timeWait' | 'other';
export type Exposure = 'loopback' | 'allInterfaces' | 'interface';

export type NetOwner = { kind: 'process'; pid: number; name: string; path: string | null } | { kind: 'unknown'; reason: string };

export interface ListenerRow {
  proto: NetProto;
  ipv6: boolean;
  local: string;
  port: number;
  exposure: Exposure;
  owner: NetOwner;
  firstSeen: Timestamp | null;
}

export interface ConnectionRow {
  proto: NetProto;
  ipv6: boolean;
  local: string;
  localPort: number;
  remote: string;
  remotePort: number;
  remoteIsLoopback: boolean;
  /** Whether "Look up IP" applies (false for private, loopback, link-local and CGNAT peers). */
  lookupOk: boolean;
  state: SocketState;
  stateLabel: string;
  owner: NetOwner;
  firstSeen: Timestamp | null;
  /** The ISO country the location database gives the remote address; null when none is loaded or it does not know it. */
  country: string | null;
  /** The names of your lists that flag the remote address (empty: none does). A flag is a hint, not proof. */
  flaggedBy: string[];
}

export interface NetworkQuery {
  showLoopback: boolean;
  proto: NetProto | null;
  state: SocketState | null;
  exposedOnly: boolean;
  search: string;
  /** Only connections to this country (ISO code, from the location database). */
  country: string | null;
}

/** Other accounts' programs as one total (bytes since each program started). */
export interface OthersTraffic {
  programs: number;
  bytesIn: number;
  bytesOut: number;
}

export interface NetworkSnapshot {
  at: Timestamp;
  listeners: ListenerRow[];
  connections: ConnectionRow[];
  truncated: number;
  unknownOwners: number;
  note: string | null;
  /** Bytes per second per program, where the OS gives them cheaply (macOS); empty otherwise. */
  traffic?: TrafficRow[];
  /** Why `traffic` is empty (shown as a one-line note), or null when it is available. */
  trafficNote?: string | null;
  /** Who read the per-program traffic. */
  trafficVia: ReadVia;
  /** What other accounts' programs moved together, for a caller the helper scopes to its own programs (no names). */
  trafficOthers?: OthersTraffic | null;
  /** Who read the socket owners. */
  ownersVia: ReadVia;
}

/** What one program sent and received in the last interval. */
export interface TrafficRow {
  pid: number;
  name: string;
  rxBps: number;
  txBps: number;
}

/** A reverse-DNS answer for an address (`name` is null when the address has no name or the lookup timed out). */
export interface HostName {
  address: string;
  name: string | null;
}

/** `Settings.networkAlerts`. */
export interface NetworkAlerts {
  newExposedListener: boolean;
  newRemoteHost: boolean;
}

/** What a custom alert rule measures; every value is a plain number in the unit the page shows. */
export type CustomMetric = 'cpuTotal' | 'memoryUsed' | 'memoryPressure' | 'swap' | 'diskFree' | 'gpuLoad' | 'temperature' | 'battery';
export type CustomOp = 'above' | 'below';

/** `Settings.customAlerts` item: raise an alarm when `metric` is `op` `limit` for `forSecs` seconds. */
export interface CustomAlertRule {
  id: string;
  name: string;
  metric: CustomMetric;
  op: CustomOp;
  /** Percent for the percentage metrics, degrees Celsius for `temperature`. */
  limit: number;
  /** 0..3600: how long the condition must hold before the alarm is raised. */
  forSecs: number;
  severity: Severity;
  enabled: boolean;
}

/** `Settings.ipLookup`: https templates; the browser opens them, Dyno never fetches. */
export interface IpLookupSettings {
  ipTemplate: string;
  asnTemplate: string;
}

// ===================================================================================================================
// END rust-agent additions. Also appended to `Settings` (all optional in the JSON): alarms,
// flightRecorder (see the widgets, alarms and recorder blocks above).
// ===================================================================================================================

// ------------------------------------------------------------------------------------- performance counters (WP-12)

/** What a counter's value measures (after any rate conversion). */
export type CounterUnit = 'percent' | 'bytes' | 'bytesPerSec' | 'perSec' | 'count' | 'milliseconds' | 'seconds' | 'number';

/** A level (`gauge`) or a running total that the engine turns into a rate per second (`cumulative`). */
export type CounterKind = 'gauge' | 'cumulative';

export interface CounterInfo {
  name: string;
  unit: CounterUnit;
  kind: CounterKind;
  help: string | null;
}

/** One object with its counters and instances (opened from the browser's first level). */
export interface CounterObject {
  name: string;
  counters: CounterInfo[];
  /** At most 100; `instancesTotal` says how many the OS has. */
  instances: string[];
  instancesTotal: number;
}

export interface CounterObjectSummary {
  name: string;
  counters: number;
  instancesTotal: number;
}

/** The browser's first level: names and sizes only. */
export interface CounterObjectList {
  objects: CounterObjectSummary[];
  /** Something was left out to keep the catalog small. */
  truncated: boolean;
  /** When the list was last read, when this answer is the one an earlier run saved (shown at once while the agent lists again); `null` when read just now. */
  savedAt: Timestamp | null;
}

/** One search match: a counter (of one instance). Paths are `\Object(Instance)\Counter`. */
export interface CounterHit {
  path: string;
  object: string;
  counter: string;
  instance: string | null;
  unit: CounterUnit;
}

/** One counter read now; unavailable carries the reason (gone, needs a second reading, refused...). */
export interface CounterReading {
  path: string;
  unit: CounterUnit;
  value: Reading<number>;
  /** 0 to 100 (clamped) for a percent counter with a value, else null: the engine's. */
  percent: number | null;
  /** Against the engine's counter thresholds; `ok` for anything but a percent counter. */
  tone: 'ok' | 'warn' | 'bad';
}

export interface CounterPoint {
  at: number;
  value: number;
}

export interface CounterSeries {
  path: string;
  unit: CounterUnit;
  points: CounterPoint[];
}

/** The readings kept for counters over the last minutes, with the app's events in the same window. */
export interface CounterHistory {
  from: number;
  to: number;
  series: CounterSeries[];
  events: TimelineEvent[];
}

export interface CounterSet {
  name: string;
  paths: string[];
}

export interface CounterSettings {
  pinned: string[];
  sets: CounterSet[];
}

// ===================================================================================================================
// WP-23 throttling, WP-24 disk and battery life, WP-25 "Is this normal?" (mirrors dyno_core::{throttle, lifespan,
// baseline, hardware}). Requests are `throttling`, `lifespan`, `baselines`; each answers the matching overview.
// ===================================================================================================================

/** How sure Dyno is of a finding; always shown in words next to it. */
export type Confidence = 'low' | 'medium' | 'high';

export type ThrottleCause = 'thermal' | 'power' | 'batterySaver' | 'unexplained';

export type ThrottleVerdict =
  | { throttled: { cause: ThrottleCause; lostPercent: number } }
  | 'fine'
  | 'notBusy'
  | 'cannotTell';

export interface ThrottleReading {
  verdict: ThrottleVerdict;
  confidence: Confidence;
  headline: string;
  explanation: string;
  evidence: Evidence[];
}

export interface ThrottleEpisode {
  /** Stable: names the episode by when it began (`throttle-<ms>`). */
  id: string;
  start: Timestamp;
  /** Null while it continues. */
  end: Timestamp | null;
  cause: ThrottleCause;
  avgLostPercent: number;
  peakLostPercent: number;
  peakTemperatureC: number | null;
}

/** What this machine reports, so "cannot tell" is explained. */
export interface InputStatus {
  label: string;
  available: boolean;
  detail: string;
}

export interface ThrottleOverview {
  at: Timestamp;
  reading: ThrottleReading;
  /** The running episode, once it has lasted long enough to count. */
  current: ThrottleEpisode | null;
  /** The last 30 days, newest first. */
  episodes: ThrottleEpisode[];
  minutes30d: number;
  inputs: InputStatus[];
}

export type LifeSubject = 'ssd' | 'battery';

export type LifeOutcome =
  | { remaining: { lowDays: number; midDays: number; highDays: number } }
  | 'beyondHorizon'
  | 'noWearDetected'
  | 'alreadyPastLimit'
  | { tooLittleHistory: { haveDays: number; needDays: number } }
  | { unavailable: { reason: string } };

export interface LifeForecast {
  subject: LifeSubject;
  name: string;
  outcome: LifeOutcome;
  confidence: Confidence;
  /** One line with the range in words ("Replace in about 22 months (17 to 31 months)"). */
  headline: string;
  explanation: string;
  /** The data the estimate stands on. */
  basis: Evidence[];
  historyDays: number;
  readings: number;
  /** Set when the series was cut because a different disk or battery appeared. */
  restartedAt: Timestamp | null;
}

export interface LifespanOverview {
  at: Timestamp;
  forecasts: LifeForecast[];
}

export type BaselineMetric = 'cpu' | 'memory' | 'diskWrite' | 'network' | 'temperature' | 'gpu' | 'appCpu' | 'appMemory';

export type Learning = 'off' | { learning: { day: number; daysNeeded: number; since: Timestamp | null; readyAt: Timestamp | null } } | { ready: { days: number } };

export type BaselineStrength = 'notable' | 'strong';

export type BaselineState = 'notJudged' | 'noBaseline' | 'usual' | { higher: BaselineStrength } | { lower: BaselineStrength };

export interface Judgement {
  state: BaselineState;
  confidence: Confidence;
  /** "Higher than usual for Thursday afternoons". */
  phrase: string;
  explanation: string;
  /** The usual value, formatted. */
  usual: string;
}

export interface BaselineItem {
  /** Stable: the metric and the subject (`cpu:System`, `appCpu:Chrome`). */
  id: string;
  /** "System" or an app's name. */
  subject: string;
  metric: BaselineMetric;
  label: string;
  current: string;
  judgement: Judgement;
}

export interface BaselineOverview {
  at: Timestamp;
  learning: Learning;
  /** Unusual values first. Empty while learning. */
  items: BaselineItem[];
}

export interface BaselineSettings {
  enabled: boolean;
  /** 1..30 days of history before anything is highlighted. */
  learningDays: number;
  /** Alarm when something stays far above its usual (off by default). */
  alarms: boolean;
}

export interface HardwareAlarms {
  throttling: boolean;
  lifespan: boolean;
}

// ---------------------------------------------------------------------------------------------------------- issues
// Mirrors `dyno_core::issues` (WP-06). Observe-only: problems the OS itself recorded, shown as plain text.

export type IssueKind =
  | 'appCrash'
  | 'appHang'
  | 'badShutdown'
  | 'diskError'
  | 'memoryError'
  | 'overheating'
  | 'serviceFailure'
  | 'memoryExhaustion'
  | 'updateFailure'
  | 'driverFailure';

export type IssueSourceId = 'macReports' | 'macLog' | 'windowsEvents' | 'journal' | 'coredumps';

export interface Issue {
  id: string;
  at: Timestamp;
  kind: IssueKind;
  /** How serious one occurrence of its kind is (decided by the engine). */
  severity: Severity;
  /** The application, service or device it concerns; empty when none applies. */
  app: string;
  title: string;
  /** The OS's own words (bounded plain text: shown with text interpolation, never as HTML). */
  evidence: string;
  source: IssueSourceId;
}

/** How a log source was last read. */
export type SourceState = { read: number } | { unavailable: string } | 'pending';

export interface SourceStatus {
  source: IssueSourceId;
  label: string;
  state: SourceState;
  at: Timestamp | null;
}

export type Stability = 'stable' | 'someProblems' | 'unstable';

export interface DayScore {
  /** The start of the local day. */
  day: Timestamp;
  score: number;
  issues: number;
}

export interface AppTally {
  name: string;
  count: number;
  last: Timestamp;
  kind: IssueKind;
}

export interface KindTally {
  kind: IssueKind;
  count: number;
}

export interface IssuesQuery {
  /** 1 to 90. */
  days: number;
}

export interface IssuesOverview {
  from: Timestamp;
  to: Timestamp;
  days: number;
  /** Today's stability score; null when no source could be read. */
  score: number | null;
  rating: Stability | null;
  daily: DayScore[];
  trend: { current: number; previous: number };
  apps: AppTally[];
  kinds: KindTally[];
  issues: Issue[];
  total: number;
  sources: SourceStatus[];
  scannedAt: Timestamp | null;
}

export interface SavedIssues {
  path: string;
  bytes: number;
  count: number;
}

// ------------------------------------------------------------------------------------------------ unusual programs
// Mirrors `dyno_core::unusual` (WP-07). A transparency tool: reasons, never verdicts.

export type UnusualLevel = 'new' | 'unusual';
export type UnusualStatus = 'new' | 'known';
export interface UnusualItem {
  /** What an action refers to. */
  key: string;
  name: string;
  exe: string | null;
  publisher: string | null;
  firstSeen: Timestamp;
  /** 0 when it was found only through a new startup entry and has not run since. */
  lastSeen: Timestamp;
  running: boolean;
  level: UnusualLevel;
  score: number;
  status: UnusualStatus;
  /** Plain words that follow "Unusual because it": "runs from a temporary folder". */
  reasons: string[];
}

export interface UnusualOverview {
  /** The engine's clock when the list was built; ages are measured against it. */
  at: Timestamp;
  enabled: boolean;
  learning: { active: boolean; until: Timestamp | null };
  learningDays: number;
  known: number;
  items: UnusualItem[];
}

/** `Settings.unusual`. */
export interface UnusualSettings {
  enabled: boolean;
  /** 0 to 60 days of silent learning. */
  learningDays: number;
}

// ------------------------------------------------------------------------------- right-click search, hash search, startup filter
// Mirrors `dyno_core::weblinks` and `dyno_core::startup` (WP-09, WP-33, WP-34). Dyno never fetches or uploads anything: the
// shell opens the browser on the click, with an address the engine built from a typed value.

/** The starting text of a web search and what was removed from it. */
export interface SearchSeed {
  /** Redacted: home folders, user and computer names, addresses, GUIDs and serial numbers are gone. Editable before searching. */
  text: string;
  /** In words: "1 home folder", "2 IP addresses". Empty when nothing was removed. */
  removed: string[];
}

/** Where the file to hash comes from. */
export type HashSource = { process: ProcessRef } | { path: string };

/** A file's SHA-256, computed on this computer. */
export interface FileHashInfo {
  path: string;
  bytes: number;
  /** 64 lower-case hexadecimal digits. */
  sha256: string;
}

/** What the startup manager hides. */
export interface StartupFilter {
  hideOs: boolean;
  hideSigned: boolean;
}

/** The startup list after the filter, with the counts the toggles show (independent of the filter). */
export interface StartupOverview {
  entries: StartupEntry[];
  total: number;
  osCount: number;
  signedCount: number;
  /** What could not be read (the list is then partial). */
  unavailable: string[];
  /** Who read the login items and background tasks (macOS). */
  btmVia: ReadVia;
}

export type ChangeKind = 'installed' | 'updated' | 'removed' | 'startupAdded' | 'startupRemoved';

export interface Change {
  at: Timestamp;
  kind: ChangeKind;
  title: string;
  detail: string;
}

/** The window of "What changed": `hours` back from `around` (or from now); 0 means the default 72. */
export interface ChangesQuery {
  around?: Timestamp | null;
  hours: number;
}

export interface ChangeTimeline {
  from: Timestamp;
  to: Timestamp;
  /** Newest first. */
  changes: Change[];
  /** When Dyno started keeping the record, when known. */
  watchingSince: Timestamp | null;
}

export type Prevents = 'sleep' | 'displaySleep' | 'other';

export interface SleepBlocker {
  name: string;
  pid: number | null;
  prevents: Prevents;
  reason: string;
  /** How long it has held on, as the tool printed it ("00:12:03"). */
  heldFor: string | null;
}

export interface WakeEvent {
  /** When, as the tool printed it (local time). */
  when: string | null;
  reason: string;
}

export interface SleepReport {
  blockers: SleepBlocker[];
  /** Newest first. */
  wakes: WakeEvent[];
  blockersUnavailable: string | null;
  wakesUnavailable: string | null;
  /** Who read the blockers. */
  blockersVia: ReadVia;
}

export interface BootStep {
  name: string;
  secs: number;
}

export interface BootRow {
  at: Timestamp;
  bootSecs: number | null;
  readySecs: number | null;
}

export interface BootReport {
  /** Newest first. */
  boots: BootRow[];
  /** What slowed the last start-up, slowest first. */
  slowest: BootStep[];
  lastTotalSecs: number | null;
  verdict: string | null;
  stepsUnavailable: string | null;
}

export interface AppUsageRow {
  app: string;
  bytesIn: number | null;
  bytesOut: number | null;
  energyImpact: number | null;
}

export interface UsageReport {
  apps: AppUsageRow[];
  networkUnavailable: string | null;
  energyUnavailable: string | null;
}

export interface GpuProcess {
  pid: number;
  name: string;
  gpuPercent: number;
  memoryBytes: number | null;
}

export interface GpuReport {
  processes: GpuProcess[];
  source: string | null;
  unavailable: string | null;
}

export interface SoftwareItem {
  name: string;
  version: string | null;
  publisher: string | null;
}

export interface SoftwareInventory {
  items: SoftwareItem[];
  truncated: boolean;
  unavailable: string | null;
  /** When the list was last read, when this answer is the one an earlier run saved (shown at once while the agent reads again); `null` when read just now. */
  savedAt: Timestamp | null;
}

export type InventoryFormat = 'markdown' | 'json';

/** The inventory report that was written. */
export interface SavedInventory {
  path: string;
  bytes: number;
}

export type FinderMode = 'large' | 'duplicates' | 'both';

export interface FinderQuery {
  /** The folder the person picked (absolute). */
  folder: string;
  mode: FinderMode;
  /** Ignore files smaller than this many bytes (the engine enforces a floor of 1 MiB). */
  minBytes: number;
}

export interface FoundFile {
  /** Id within the result; a selection refers to it. */
  id: string;
  path: string;
  bytes: number;
  modified: Timestamp;
}

export interface DuplicateGroup {
  bytesEach: number;
  wastedBytes: number;
  /** The copies, oldest first (the oldest is the one kept by default). */
  files: FoundFile[];
}

export interface FinderResult {
  /** Id of this result; a removal refers to it and it expires after an hour. */
  scanId: string;
  folder: string;
  large: FoundFile[];
  duplicates: DuplicateGroup[];
  scanned: number;
  /** Folders the walk could not read (no permission, a share that went away); they are left out. */
  unreadable: number;
  /** The limit that stopped the scan early, with the sentence to show; `null` when it read everything. */
  limit: FinderLimitHit | null;
  wastedBytes: number;
}

export type FinderLimit = 'files' | 'depth' | 'hashBytes';

/** A limit that stopped a scan early. The engine words the message; the page only shows it. */
export interface FinderLimitHit {
  limit: FinderLimit;
  message: string;
}

// ------------------------------------------------------------------------------------------------------------- jobs

/**
 * Long operations (find files, read the system logs, check the backups, hash a file, the sleep, boot, app-usage and software
 * reads) run as jobs in the agent: starting one answers at once with its id, the shell's jobs store follows it through the
 * `paddock://jobs` events, Cancel is the shell's `job_cancel`, and `jobResult` hands over what a done job produced. Mirrors
 * `dyno_core::jobs`; the status itself is the shell's `SuJobSnapshot`.
 */
export type JobSpec =
  | { findFiles: { query: FinderQuery } }
  | { rescanIssues: { query: IssuesQuery } }
  | 'refreshBackup'
  | { fileHash: { source: HashSource } }
  | 'sleepBlockers'
  | 'bootAnalysis'
  | 'appUsage'
  | 'softwareInventory'
  | { helper: { action: HelperAction } }
  | { updateList: { id: string } }
  | 'updateGeo'
  | 'rebuildIntel'
  | { installTool: { tool: ToolKind } }
  | { wolWake: { id: number; test: boolean } };

/** What a done job produced, tagged by the kind of job. */
export type JobOutput =
  | { intel: IntelStatus }
  | { finder: FinderResult }
  | { issues: IssuesOverview }
  | { backup: BackupReport }
  | { fileHash: FileHashInfo }
  | { sleep: SleepReport }
  | { boot: BootReport }
  | { appUsage: UsageReport }
  | { software: SoftwareInventory }
  | { helper: HelperStatus }
  | { tool: ToolStatus }
  | { wol: WolOutcome };

// ------------------------------------------------------------------------------------- the privileged helper (ADR 0027)

/**
 * Whether the helper is usable. `notIncluded`: this build or system has no helper (an AppImage). `off`: never turned on.
 * `notRunning`: installed but not answering. `missing`: damaged. `foreign`: another program answers where the helper should be
 * (never trusted, never sent anything). `outdated`: needs the bundled newer helper. `ready`: running.
 */
export type HelperState = 'notIncluded' | 'off' | 'notRunning' | 'missing' | 'foreign' | 'outdated' | 'ready';

/** What the user asks of the helper; each runs as a job with phases. */
export type HelperAction = 'turnOn' | 'repair' | 'update' | 'turnOff';

/** How an audited call ended. */
export type AuditVerdict = 'done' | 'refused' | 'failed';

/** One line of the helper's audit log. */
export interface AuditEntry {
  /** Seconds since 1970. */
  at: number;
  peer: number | null;
  what: string;
  verdict: AuditVerdict;
  detail: string;
}

/** Reply of `helperStatus`: the state, one sentence for the person, and what the OS will ask. */
export interface HelperStatus {
  state: HelperState;
  message: string;
  version: string | null;
  /** What the OS asks for when turning it on, in words (shown before the prompt). */
  prompt: string;
  recent: AuditEntry[];
  /** What the helper shows, for the card's list (the same list whether it is off or on); empty where the helper cannot exist. */
  readings: string[];
  /** Technical details for a "Details" disclosure (versions, an owner's account id, the OS's words); never the headline. */
  detail: string | null;
  error: string | null;
}

export type Weekday = 'mon' | 'tue' | 'wed' | 'thu' | 'fri' | 'sat' | 'sun';

/** Some weekdays, from a local time to a local time; an end at or before the start runs past midnight. */
export interface MaintenanceWindow {
  days: Weekday[];
  /** Minutes after local midnight (0 to 1439). */
  startMinute: number;
  /** Minutes after local midnight (0 to 1439); not equal to the start. */
  endMinute: number;
}

export interface MaintenanceSettings {
  enabled: boolean;
  windows: MaintenanceWindow[];
}

export interface MaintenanceStatus {
  enabled: boolean;
  openNow: boolean;
  closesAt: Timestamp | null;
  nextOpensAt: Timestamp | null;
}

// ------------------------------------------------------------------------------------------------------- backup
// Mirrors `dyno_core::backup` (WP-14). Dyno reads each tool's own state with timeouts; it never runs or manages a backup.

export type BackupTool = 'timeMachine' | 'windowsBackup' | 'fileHistory' | 'restic' | 'borg' | 'duplicati';

export type BackupState = 'healthy' | 'stale' | 'noBackupYet' | 'notConfigured' | 'cannotCheck';

export interface BackupStatus {
  tool: BackupTool;
  name: string;
  state: BackupState;
  /** When the last successful backup finished, when known. */
  lastBackup: Timestamp | null;
  summary: string;
  nextStep: string | null;
}

export interface BackupReport {
  statuses: BackupStatus[];
  /** When it was checked; null while the first check is still running. */
  checkedAt: Timestamp | null;
  checking: boolean;
  loginPathUsed: boolean;
  /** Who read the Windows Backup versions. */
  via: ReadVia;
}

/** Folders to look in before the PATH for one tool installed somewhere unusual. */
export interface ToolOverride {
  tool: BackupTool;
  /** Absolute folders, searched first. */
  dirs: string[];
}

export interface BackupSettings {
  /** Check in the background and raise an alarm for a stale backup. */
  alertWhenStale: boolean;
  /** A backup older than this many hours is stale (1 to 2160). */
  staleAfterHours: number;
  overrides: ToolOverride[];
}

// ---------------------------------------------------------------------------------------------- IP lists, the map, the firewall

/** The switches of the network features: on by default, each its own (there is no Offline mode). */
export interface NetSwitches {
  locationDatabases: boolean;
  ownLocation: boolean;
  firstRunNoteSeen: boolean;
}

/** When a subscription is refreshed. */
export type ListSchedule = 'manual' | { automatic: { every: number } };

/** Where a list comes from. */
export type ListSource = 'manual' | { file: { name: string } } | { url: { url: string; schedule: ListSchedule } };

/** What the page shows for a subscription (and the counts of a typed list). */
export interface SubscriptionStatus {
  lastFetchedMs: number | null;
  lastAttemptMs: number | null;
  entries: number;
  duplicates: number;
  covered: number;
  skipped: number;
  error: string | null;
  note: string | null;
}

/** One list. */
export interface ListStatus {
  id: string;
  name: string;
  enabled: boolean;
  source: ListSource;
  counts: SubscriptionStatus;
  /** When a subscription may be updated again (ms since the epoch), when later than now. */
  nextAllowedMs: number | null;
  /** The text of a typed or imported list for the editor (at most 64 KB); null for a subscription or a larger list. */
  text: string | null;
}

/** The location features. */
export interface GeoStatus {
  switches: NetSwitches;
  databaseOn: boolean;
  dbDate: string | null;
  country: SubscriptionStatus;
  asn: SubscriptionStatus;
  homeSource: string;
  attribution: string;
}

/** Everything the lists page shows. */
export interface IntelStatus {
  lists: ListStatus[];
  allow: string;
  indexSegments: number;
  indexRanges: number;
  indexBytes: number;
  indexError: string | null;
  geo: GeoStatus;
  firstRunNote: string;
  termsNotice: string;
}

/** How a new list starts. */
export type NewSource =
  | { manual: { text: string } }
  | { file: { fileName: string; text: string } }
  | { url: { url: string; everyHours: number | null } };

/** A change the user asks for. */
export type IntelEdit =
  | { addList: { name: string; source: NewSource } }
  | { removeList: { id: string } }
  | { renameList: { id: string; name: string } }
  | { setEnabled: { id: string; enabled: boolean } }
  | { setText: { id: string; text: string } }
  | { setSchedule: { id: string; everyHours: number | null } }
  | { dedupePreview: { id: string } }
  | { dedupeApply: { id: string } }
  | { setAllow: { text: string } }
  | { setSwitches: { switches: NetSwitches } };

/** What "Remove duplicates" would do. */
export interface DedupePreview {
  before: number;
  after: number;
  duplicates: number;
  covered: number;
  /** `[removed entry, the entry that stays and covers it]`. */
  examples: [string, string][];
}

/** What an edit produced. */
export type EditOutcome = 'done' | { added: { id: string } } | { preview: DedupePreview } | { removed: { id: string } };

/** The class of a non-public destination. */
export type AddressClass = 'public' | 'loopback' | 'private' | 'linkLocal' | 'cgnat' | 'multicast' | 'special' | 'reserved';

/** One country on the map. */
export interface CountryCount {
  code: string;
  name: string;
  connections: number;
  programs: number;
  hosts: number;
  flagged: number;
  includesContentNetworks: boolean;
  /** The programs with most connections to it (at most three, busiest first). */
  topPrograms: string[];
}

/** Non-public destinations of one class. */
export interface LocalGroup {
  class: AddressClass;
  connections: number;
  programs: number;
}

/** The map's data. */
export interface GeoSummary {
  countries: CountryCount[];
  local: LocalGroup[];
  unknown: number;
  dbDate: string | null;
  truncated: number;
  databaseOn: boolean;
  homeCountry: string | null;
  homeSource: string;
}

export type RuleDirection = 'in' | 'out' | 'any';
export type RuleAction = 'allow' | 'block';
export type FirewallProfile = 'domain' | 'private' | 'public' | 'all';
export type RuleSourceKind = 'windowsFirewall' | 'applicationFirewall' | 'pf' | 'nftables';

export interface FirewallRule {
  name: string;
  direction: RuleDirection;
  action: RuleAction;
  enabled: boolean;
  program: string | null;
  ports: string | null;
  addresses: string | null;
  profile: FirewallProfile;
  source: RuleSourceKind;
}

export interface FirewallSource {
  source: RuleSourceKind;
  states: { profile: FirewallProfile; enabled: boolean }[];
  switches: [('blockAll' | 'stealth'), boolean][];
  defaultIn: RuleAction | null;
  defaultOut: RuleAction | null;
  ruleCount: number;
  rules: FirewallRule[];
  truncated: number;
  rulesHidden: boolean;
  note: string | null;
  /** Who read it. Always sent; `local` for a reading that never needed the helper. */
  via: ReadVia;
}

/** The firewall of this computer. */
export interface FirewallView {
  os: 'windows' | 'macos' | 'linux';
  sources: FirewallSource[];
}

// ------------------------------------------------------------------------------------- the disk-health tool (ADR 0027 section 7)

/** A tool the helper can hold. */
export type ToolKind = 'smartmontools';

/** What the helper holds of one tool. */
export interface ToolStatus {
  tool: ToolKind;
  installed: string | null;
}

/** Where the offer stands: downloadable, installed (the version), needs the helper first, or not offered here (what to do instead). */
export type OfferState = 'available' | { installed: string } | { needsHelper: string } | { notHere: string };

/** Which copy of the tool Dyno uses: the helper's, Dyno's own, or one the person installed (the path of its smartctl). */
export type ToolSource = 'helper' | 'dyno' | { own: string };

/** What the Settings card shows for the disk-health tool. */
export interface ToolOffer {
  tool: ToolKind;
  name: string;
  version: string;
  size: number;
  from: string;
  licence: string;
  sourceUrl: string;
  state: OfferState;
  /** Which copy Dyno uses, when it is installed. */
  source?: ToolSource | null;
}

// ---------------------------------------------------------------------------------------------------- power (ADR 0032)

/** What stays awake. */
export type AwakeScope = 'system' | 'systemAndDisplay';

/** One "busy" condition of a "while busy" hold (externally tagged, like the engine's enums: postcard carries them). */
export type BusyCond =
  | { cpu: { abovePct: number } }
  | { network: { aboveBytesPerS: number } }
  | { disk: { aboveBytesPerS: number } }
  | { process: { program: string } };

/** Until when to keep awake. */
export type AwakeUntil =
  | 'indefinite'
  | { for: { minutes: number } }
  | { at: { minuteOfDay: number } }
  | { whileProcess: { process: ProcessRef } }
  | { whileBusy: { conds: BusyCond[]; graceSecs: number } };

/** A keep-awake request; one is active at a time and a new one replaces it. */
export interface AwakeSpec {
  scope: AwakeScope;
  until: AwakeUntil;
}

/** Where a "while busy" hold stands: busy now (and why) or quiet, with the time left before it lets go. */
export interface BusyNow {
  active: boolean;
  detail: string;
  quietSecs: number;
  releaseInSecs: number | null;
}

/** The keep-awake state. */
export type AwakeView =
  | 'off'
  | { holding: { spec: AwakeSpec; since: Timestamp; ends: Timestamp | null; osRef: string; busy: BusyNow | null } }
  | { waiting: { spec: AwakeSpec; why: string; since: Timestamp; ends: Timestamp | null } }
  | { ended: { why: string; at: Timestamp } };

/** Whether a power feature works here; the UI never guesses. */
export type Capability = 'known' | { cantTell: { reason: string } };

/** Everything the Power page shows. */
export interface PowerView {
  awake: AwakeView;
  idle: IdleView;
  caps: { keepAwake: Capability; sleep: Capability; hibernate: Capability; shutDown: Capability };
}

export interface KeepAwakeSettings {
  enabled: boolean;
  display: boolean;
  graceSecs: number;
  cpuPct: number;
  netBytesPerS: number;
  diskBytesPerS: number;
  trayItem: boolean;
}

export interface EnergySettings {
  enabled: boolean;
  wattsOn: number | null;
  wattsSleep: number | null;
  tariffPerKwh: number | null;
  currency: string | null;
  co2GPerKwh: number | null;
}

export type IdleAction = 'sleep' | 'hibernate' | 'shutDown';

export interface IdleExceptions {
  audio: boolean;
  fullScreen: boolean;
  otherStayAwake: boolean;
  cpuPct: number | null;
  diskBytesPerS: number | null;
  netBytesPerS: number | null;
  programs: string[];
}

export interface IdleSettings {
  enabled: boolean;
  action: IdleAction;
  afterSecs: number;
  countdownSecs: number;
  acOnly: boolean;
  exceptions: IdleExceptions;
}

/** How often a timed action repeats (externally tagged like the engine's enum). */
export type Repeat = 'once' | 'daily' | { weekdays: number };

export interface TimedAction {
  id: number;
  action: IdleAction;
  atMinute: number;
  repeat: Repeat;
  lastDay: number | null;
  done: boolean;
}

export interface TimedSettings {
  enabled: boolean;
  entries: TimedAction[];
}

export interface WolSettings {
  enabled: boolean;
  discovery: boolean;
  directedBroadcast: boolean;
  testTimeoutSecs: number;
}

export interface WakeSettings {
  enabled: boolean;
}

export interface PowerSettings {
  keepAwake: KeepAwakeSettings;
  energy: EnergySettings;
  idle: IdleSettings;
  timed: TimedSettings;
  wol: WolSettings;
  wake: WakeSettings;
}

// ------------------------------------------------------------------------------------------ scheduled wake (ADR 0032 section 5)

/** When a scheduled wake repeats: once, every day, or on weekdays (bit 0 Monday to bit 6 Sunday). */
export type WakeRepeat = 'once' | 'daily' | { days: { mask: number } };

/** The jobs a schedule may run: a fixed list, never a command. */
export type DynoJob = 'backupCheck' | 'softwareInventory';

export interface WakeLastRun {
  at: Timestamp;
  summary: string;
}

/** A saved schedule. */
export interface WakeSchedule {
  id: number;
  name: string;
  minuteOfDay: number;
  repeat: WakeRepeat;
  stayAwakeMinutes: number | null;
  wakeDevices: number[];
  job: DynoJob | null;
  enabled: boolean;
  nextAt: Timestamp | null;
  lastRun: WakeLastRun | null;
}

/** What the person chose when adding or changing a schedule. */
export interface WakeInput {
  id: number | null;
  name: string;
  minuteOfDay: number;
  repeat: WakeRepeat;
  stayAwakeMinutes: number | null;
  wakeDevices: number[];
  job: DynoJob | null;
  enabled: boolean;
}

export type WakeRoute = 'agent' | 'helper';

/** How a schedule's row is doing (externally tagged). */
export type WakeRowState = 'off' | 'paused' | 'needsHelper' | { armed: { route: WakeRoute } } | { failed: { why: string } };

export interface WakeRow {
  entry: WakeSchedule;
  state: WakeRowState;
}

/** Who sets the OS wake here and whether it can be done now. */
export type WakeRouteState = 'helperReady' | { agent: { note: string; cantTell: string[] } } | { needsHelper: { reason: string } } | { unavailable: { reason: string } };

/** What the Schedule tab shows. */
export interface WakesView {
  rows: WakeRow[];
  route: WakeRouteState;
}

export type BlockerKind = 'audio' | 'fullScreen' | 'stayAwake' | 'cpu' | 'disk' | 'network' | 'program' | 'battery';

/** One reason the action waits; `cantTell` means Dyno could not read it, which blocks too. */
export interface Blocker {
  kind: BlockerKind;
  reason: string;
  cantTell: boolean;
}

/** The idle and timed actions' state (externally tagged). */
export type IdleView =
  | 'disabled'
  | { unavailable: { reason: string } }
  | { watching: { idleSecs: number; blockers: Blocker[]; snoozedUntil: Timestamp | null } }
  | { countdown: { action: IdleAction; deadline: Timestamp; cause: 'idle' | 'timed' } }
  | { acting: { action: IdleAction; since: Timestamp } }
  | { stopped: { why: string } };

/** One day (or the whole range) of the energy report. */
export interface EnergyDay {
  dayStart: Timestamp;
  onSecs: number;
  sleepSecs: number;
  /** Seconds Dyno was not running, so it does not know. */
  unknownSecs: number;
  kwh: number | null;
  cost: number | null;
  co2Kg: number | null;
  /** CPU energy the helper measured (the processor's alone, not the machine's); null when nothing was measured. */
  cpuKwh: number | null;
}

/** The energy report: the person's own watts times the time Dyno saw, labelled as an estimate. */
export interface EnergyReport {
  enabled: boolean;
  days: EnergyDay[];
  totals: EnergyDay;
  source: 'estimated' | 'notSet';
  sleepEnergyCounted: boolean;
  currency: string | null;
  trackingSince: Timestamp | null;
  /** Who measured the CPU energy: `local` where this computer is not asked, so nothing shows. */
  cpuVia: ReadVia;
}

// ------------------------------------------------------------------------------------------ Wake-on-LAN (ADR 0032 section 3)

/** How the last wake of a computer ended. */
export interface LastWake {
  at: Timestamp;
  ok: boolean;
  tested: boolean;
  summary: string;
}

/** A saved computer to wake. The SecureOn password is never here: `secureOn` only says there is one. */
export interface WolDevice {
  id: number;
  name: string;
  /** `aa:bb:cc:dd:ee:ff`. */
  mac: string;
  lastIp: string | null;
  interface: string | null;
  port: number;
  secureOn: boolean;
  lastWake: LastWake | null;
}

/** What the Wake devices tab shows. */
export interface WolView {
  enabled: boolean;
  devices: WolDevice[];
  canSend: Capability;
}

/** What to do with the SecureOn password when saving (externally tagged). */
export type SecureOnEdit = 'keep' | 'clear' | { set: { password: string } };

/** What the person typed or picked when adding or changing a computer. */
export interface WolDeviceInput {
  id: number | null;
  name: string;
  mac: string;
  lastIp: string | null;
  interface: string | null;
  secureOn: SecureOnEdit;
}

export type NeighbourState = 'reachable' | 'stale' | 'probe' | 'permanent' | 'unknown';

/** A neighbour-table entry: a computer the OS has talked to on the local network. */
export interface Neighbour {
  mac: string;
  ip: string;
  interface: string;
  state: NeighbourState;
  /** The host name the agent's resolver knows, when it knows one. */
  name: string | null;
}

export interface SendResult {
  interface: string;
  dest: string;
  bytes: number;
  sent: number;
  error: string | null;
}

export type AnswerHow = 'ping' | 'neighbour';

export interface WolAnswer {
  how: AnswerHow;
  afterSecs: number;
  newIp: string | null;
}

/** What a wake did; `message` is the result in words, with the next step when it did not work. */
export interface WolOutcome {
  sent: SendResult[];
  answered: WolAnswer | null;
  tested: boolean;
  elapsedSecs: number;
  timedOut: boolean;
  cancelled: boolean;
  message: string;
}

export type ReadinessKind = 'nicWake' | 'osSetting' | 'fastStartup' | 'sleepMode' | 'firmware';

/** One finding: fine, a problem with its fix, or "can't tell" with the reason (externally tagged). */
export type ReadinessState = { ok: { note: string } } | { problem: { note: string; fix: string } } | { cantTell: { reason: string } };

export interface ReadinessItem {
  kind: ReadinessKind;
  subject: string | null;
  state: ReadinessState;
}

/** This PC's Wake-on-LAN readiness. */
export interface ThisPcWol {
  items: ReadinessItem[];
}
