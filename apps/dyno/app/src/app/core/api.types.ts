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

export type Grade = 'good' | 'fair' | 'poor';
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
  /** The customisable dashboard (the engine fills the default Overview when absent). */
  dashboard?: DashboardLayout;
  /** The menu-bar / tray items (at most 4; the engine fills the default when absent). */
  trayItems?: TrayItems;
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
  needsHelper: boolean;
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

export type WidgetKind = 'graph' | 'bigNumber' | 'bars' | 'ring' | 'heatmap' | 'topProcesses' | 'issues' | 'health';

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

// ===================================================================================================================
// END rust-agent additions. Also appended to `Settings` (all optional in the JSON): dashboard, trayItems, alarms,
// flightRecorder (see the widgets, alarms and recorder blocks above).
// ===================================================================================================================
