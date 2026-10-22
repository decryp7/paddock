/**
 * The UI ↔ app wire contract (camelCase JSON). Mirrors the Rust types in `crates/dyno-core` (serde, externally
 * tagged enums: a unit variant is a string, a data variant is `{ variant: data }`). Change both sides together.
 */

/** Milliseconds since the Unix epoch. */
export type Timestamp = number;

/** A measurement the platform may not provide, with the reason when it doesn't. */
export type Reading<T> = { available: T } | { unavailable: string };

// ---------------------------------------------------------------------------------------------------------- metrics

export interface CpuSample {
  total: number;
  perCore: number[];
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
  /** Why Dyno never touches this process (built-in system list or the user's protected list); `null` when it may act. */
  protectedReason: string | null;
}

/** A pid plus its start time: actions never hit a reused pid. */
export interface ProcessRef {
  pid: number;
  started: Timestamp;
}

export type Priority = 'idle' | 'belowNormal' | 'normal' | 'aboveNormal' | 'high';

export type ProcessAction =
  | 'end'
  | 'suspend'
  | 'resume'
  | { setPriority: Priority }
  | { setAffinity: number }
  | { setEfficiencyMode: boolean }
  | { setCpuCap: number | null };

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

export type ProfileId = 'normal' | 'gaming' | 'meeting' | 'onBattery' | 'buildingCode';

/** Pushed every tick on the `dyno://live` event. */
export interface LiveUpdate {
  system: SystemSample;
  /** The OS's pressure level where it reports one, else derived by the engine. */
  pressure: MemoryPressure;
  topCpu: AppGroup[];
  topMemory: AppGroup[];
  overhead: Overhead;
  profile: ProfileId;
  rulesPaused: boolean;
  activeActions: ActiveAction[];
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

export type SuggestedAction =
  | { process: { target: ProcessRef; name: string; action: ProcessAction } }
  | 'openCleanup'
  | 'openTaskManager'
  | 'openStartup'
  | 'restart'
  | 'wait';

export interface Suggestion {
  label: string;
  action: SuggestedAction;
}

export interface Cause {
  kind: CauseKind;
  title: string;
  explanation: string;
  /** 0–1. */
  confidence: number;
  evidence: Evidence[];
  suggestions: Suggestion[];
}

export interface Diagnosis {
  at: Timestamp;
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

export type TimelineEventKind = 'throttle' | 'cap' | 'release' | 'cleanup' | 'tuning' | 'alert' | 'boot';

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
}

// --------------------------------------------------------------------------------------------------------- rules

export type RuleContext = 'userActive' | 'userIdle' | 'onBattery' | 'onAc';

export type RuleTarget = 'anyApp' | { apps: string[] };

export interface RuleCondition {
  target: RuleTarget;
  /** Percent of the machine. */
  cpuAbove: number;
  forSecs: number;
  when: RuleContext[];
}

export type StepAction = 'lowerPriority' | 'efficiencyMode' | { cpuCap: number } | 'notify';

export interface RuleStep {
  action: StepAction;
  /** Seconds after the previous step (or after the condition started holding) before this one is taken. */
  afterSecs: number;
}

export interface Rule {
  id: string;
  name: string;
  enabled: boolean;
  condition: RuleCondition;
  ladder: RuleStep[];
  /** Seconds below the threshold before Dyno undoes what it did. */
  releaseAfterSecs: number;
  /** Profiles the rule is active in; empty = every profile. */
  profiles: ProfileId[];
}

export interface Profile {
  id: ProfileId;
  name: string;
  description: string;
}

export interface RuleSet {
  rules: Rule[];
  /** Executable names the user protected (never touched); the built-in system list is in `builtInProtected`. */
  protected: string[];
  builtInProtected: string[];
  profile: ProfileId;
  profiles: Profile[];
  paused: boolean;
}

export interface ActiveAction {
  target: ProcessRef;
  app: string;
  ruleId: string;
  ruleName: string;
  /** Index into the rule's ladder of the last step taken. */
  step: number;
  description: string;
  since: Timestamp;
}

export type ActivityKind = 'rule' | 'release' | 'manual' | 'cleanup' | 'tuning' | 'startup' | 'alert' | 'system';

export type UndoState = 'notUndoable' | 'available' | 'undone' | 'expired';

export interface ActivityEntry {
  id: string;
  at: Timestamp;
  kind: ActivityKind;
  title: string;
  detail: string;
  undo: UndoState;
  /** Bytes freed, for cleanups. */
  bytes: number | null;
}

// ------------------------------------------------------------------------------------------------------- cleanup

export type CleanupGroup = 'system' | 'browser' | 'developer' | 'downloads' | 'docker' | 'plugin';
export type CleanupRisk = 'safe' | 'review' | 'personal';

export interface CleanupCategory {
  id: string;
  name: string;
  description: string;
  group: CleanupGroup;
  risk: CleanupRisk;
  /** Whether scheduled runs include it (the user enabled it). */
  scheduled: boolean;
}

export interface CleanupItem {
  id: string;
  path: string;
  bytes: number;
  modified: Timestamp;
  folder: boolean;
  reason: string;
  /** Pre-selected in the preview (never for `personal` categories). */
  selected: boolean;
}

export interface CategoryResult {
  category: string;
  bytes: number;
  itemCount: number;
  items: CleanupItem[];
  truncated: boolean;
  /** Why the category could not be scanned fully, e.g. "Docker is not running". */
  note: string | null;
}

export interface CleanupScan {
  id: string;
  at: Timestamp;
  results: CategoryResult[];
  totalBytes: number;
}

export type DeleteMode = 'recycleBin' | 'quarantine';

export interface FailedItem {
  path: string;
  reason: string;
}

export interface CleanupOutcome {
  freedBytes: number;
  deleted: number;
  failed: FailedItem[];
  quarantineId: string | null;
}

export interface QuarantineEntry {
  id: string;
  at: Timestamp;
  bytes: number;
  itemCount: number;
  expires: Timestamp;
}

export interface SizeNode {
  name: string;
  path: string;
  bytes: number;
  children: SizeNode[];
}

// ------------------------------------------------------------------------------------------- health and tuning

export type Grade = 'excellent' | 'good' | 'fair' | 'poor';
export type Severity = 'info' | 'warning' | 'critical';

export type FixKind = { tuning: string } | 'openCleanup' | 'openStartup' | 'restart' | 'openTaskManager';

export interface Fix {
  label: string;
  explanation: string;
  /** What will change, one line each. */
  preview: string[];
  needsAdmin: boolean;
  undoable: boolean;
  kind: FixKind;
}

export interface HealthReason {
  id: string;
  title: string;
  detail: string;
  /** Points this costs the score. */
  impact: number;
  severity: Severity;
  fix: Fix | null;
}

export interface HealthReport {
  at: Timestamp;
  score: number;
  grade: Grade;
  reasons: HealthReason[];
}

export interface TuningSuggestion {
  id: string;
  title: string;
  explanation: string;
  current: string;
  proposed: string;
  needsAdmin: boolean;
  applied: boolean;
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

export type StartupState = 'enabled' | 'disabled' | { delayed: number };

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
  /** Changing it goes through the privileged helper (machine-wide locations). */
  needsAdmin: boolean;
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
  score: number;
  scoreChange: number;
  avgCpu: number;
  peakCpu: number;
  avgMemory: number;
  topApps: Consumer[];
  ruleActions: number;
  cleanedBytes: number;
  crashes: number;
  alerts: number;
  highlights: string[];
}

// ------------------------------------------------------------------------------------------------------ settings

export type TrayMetric = 'cpu' | 'memory' | 'disk' | 'network' | 'gpu';
export type TrayStyle = 'bars' | 'graph';
export type Theme = 'system' | 'dark' | 'light';
export type CleanupSchedule = 'off' | 'daily' | 'weekly';

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
  cleanup: { schedule: CleanupSchedule; categories: string[]; mode: DeleteMode; devProjectAgeDays: number };
  webApi: { enabled: boolean; port: number; lan: boolean };
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
  | 'protected'
  | 'badRequest'
  | 'timeout'
  | 'busy'
  | 'internal';

/** What every failed backend call rejects with. */
export interface CommandError {
  code: ErrorCode;
  message: string;
}
