/**
 * The UI ↔ app wire contract (camelCase JSON). Mirrors the Rust types in `crates/dyno-core` (serde, externally
 * tagged enums: a unit variant is a string, a data variant is `{ variant: data }`). Change both sides together.
 */

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

export interface ProcessSample {
  pid: number;
  parent: number | null;
  name: string;
  exe: string | null;
  user: string | null;
  /** Percent of the whole machine, 0–100. */
  cpu: number;
  memory: number;
  diskReadBps: number;
  diskWriteBps: number;
  started: Timestamp;
  state: RunState;
}

/** A pid plus its start time: actions never hit a reused pid. */
export interface ProcessRef {
  pid: number;
  started: Timestamp;
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

export interface Locker {
  process: ProcessRef;
  name: string;
  kind: string | null;
}

/** One application with all its processes. */
export interface AppGroup {
  id: string;
  name: string;
  exe: string | null;
  pids: number[];
  cpu: number;
  memory: number;
  diskBps: number;
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
  cpu: number;
  memory: number;
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
}

export type LiveDetail = 'summary' | 'processes';

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
}

export type NodeKind = 'folder' | 'file' | 'mountPoint' | 'unreadable';

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
}

export interface HealthReport {
  at: Timestamp;
  score: number;
  grade: Grade;
  reasons: HealthReason[];
}

/** How the privileged helper stands (ADR 0016). */
export type HelperState = 'unsupported' | 'notInstalled' | 'ready' | 'outdated' | 'unreachable' | 'installing';

/** One line of the helper's own audit log. */
export interface HelperAuditEntry {
  at: number;
  peer: number | null;
  what: string;
  verdict: 'applied' | 'refused' | 'failed';
  detail: string;
}

/** The privileged helper for Settings and the "install the helper" dialog. */
export interface HelperStatus {
  state: HelperState;
  /** One sentence saying what it means and what to do next. */
  message: string;
  version: string | null;
  /** What the operating system will ask for when installing, in words. */
  prompt: string;
  /** The last entries of the helper's audit log, newest first. */
  recent: HelperAuditEntry[];
  /** Why the last install, update or removal did not work, until the next attempt. */
  error: string | null;
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

export type TrayMetric = 'cpu' | 'memory' | 'disk' | 'network' | 'gpu';
export type TrayStyle = 'bars' | 'graph';
export type Theme = 'system' | 'dark' | 'light';
export interface Settings {
  version: number;
  theme: Theme;
  launchAtLogin: boolean;
  sampling: { intervalMs: number; processIntervalMs: number };
  retention: { rawHours: number; minuteDays: number; quarterHourDays: number; hourDays: number };
  tray: { metrics: TrayMetric[]; style: TrayStyle };
  alerts: {
    enabled: boolean;
    lowDiskPercent: number;
    overheatCelsius: number;
    longHighCpuMinutes: number;
  };
  webApi: { enabled: boolean; port: number; lan: boolean };
  /** The customisable dashboard (the engine fills the default Overview when absent). */
  dashboard?: DashboardLayout;
  /** The menu-bar / tray items (at most 4; the engine fills the default when absent). */
  trayItems?: TrayItems;
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
  /** Maintenance windows: when Dyno's own heavier background work may run. */
  maintenance: MaintenanceSettings;
  /** Backup health checks. */
  backup: BackupSettings;
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
  | 'internal';

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
  /** Reading it needs the privileged helper (a later work package). */
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
}

// --------------------------------------------------------------------------------------------------------- widgets
// `Settings` gains `dashboard: DashboardLayout` and `trayItems: TrayItems` (both optional in the JSON; defaults fill in).

export type WidgetKind = 'graph' | 'bigNumber' | 'bars' | 'ring' | 'heatmap' | 'topProcesses' | 'issues' | 'health' | 'cpu' | 'diagnosis' | 'events' | 'history' | 'consumers' | 'diskTrend';

/** Externally tagged: unit variants are strings, the two with an argument are single-key objects. */
export type WidgetMetric =
  | 'cpuTotal'
  | 'cpuPerCore'
  | 'memory'
  | 'memoryPressure'
  | 'swap'
  | 'diskActivity'
  | 'diskFree'
  | 'networkUp'
  | 'networkDown'
  | 'gpuLoad'
  | 'temperature'
  | 'fan'
  | 'battery'
  | { process: string }
  | { perfCounter: string };

export type WidgetSize = 's' | 'm' | 'l';

export interface WidgetThresholds {
  warning: number | null;
  critical: number | null;
}

export interface WidgetOptions {
  /** 30..3600 */
  rangeSecs: number;
  showLabel: boolean;
  fill: boolean;
  /** '#rrggbb' or null for the theme default. */
  accent: string | null;
}

export interface Widget {
  id: string;
  kind: WidgetKind;
  metric: WidgetMetric;
  options: WidgetOptions;
  size: WidgetSize;
  thresholds: WidgetThresholds;
}

export interface NamedPreset {
  id: string;
  name: string;
  /** Overview, Developer, Gaming, Battery: regenerated by the engine on every load. */
  builtin: boolean;
  widgets: Widget[];
}

export interface DashboardLayout {
  version: number;
  widgets: Widget[];
  presets: NamedPreset[];
}

export type TrayItemStyle =
  | 'lineGraph'
  | 'verticalBars'
  | 'horizontalMeter'
  | 'ring'
  | 'graphAndNumber'
  | { number: { unit: string; label: string } };

export type TrayOpens = 'ownPopover' | 'mainPopover';

export interface TrayItem {
  id: string;
  metric: WidgetMetric;
  style: TrayItemStyle;
  opens: TrayOpens;
}

/** At most 4 items, reorderable (array order = left to right). */
export interface TrayItems {
  version: number;
  items: TrayItem[];
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
  metric: WidgetMetric | null;
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
  /** Local offset from UTC in minutes (set from `-new Date().getTimezoneOffset()`). */
  utcOffsetMinutes: number;
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
}

export interface BenchProgress {
  runId: string;
  startedAt: number;
  kinds: BenchKind[];
  current: BenchKind | null;
  done: BenchKind[];
}

export interface BenchOverview {
  running: BenchProgress | null;
  runs: BenchRun[];
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
  verdict: BenchVerdict;
}

export interface BenchComparison {
  before: string;
  after: string;
  deltas: MetricDelta[];
  warnings: string[];
}

export interface StartBenchmark {
  kinds: BenchKind[];
  label: string;
  group?: string | null;
  phase?: BenchPhase | null;
  diskMib?: number | null;
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
}

export interface NetworkQuery {
  showLoopback: boolean;
  proto: NetProto | null;
  state: SocketState | null;
  exposedOnly: boolean;
  search: string;
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
// END rust-agent additions. Also appended to `Settings` (all optional in the JSON): dashboard, trayItems, alarms,
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

export type Learning = 'off' | { learning: { daysDone: number; daysNeeded: number } } | { ready: { days: number } };

export type BaselineStrength = 'notable' | 'strong';

export type BaselineState = 'notJudged' | 'noBaseline' | 'usual' | { higher: BaselineStrength } | { lower: BaselineStrength };

export interface Judgement {
  state: BaselineState;
  confidence: Confidence;
  /** "Higher than usual for Tuesday mornings". */
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
  /** The scan stopped early (file or time limit); more exists. */
  truncated: boolean;
  wastedBytes: number;
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
