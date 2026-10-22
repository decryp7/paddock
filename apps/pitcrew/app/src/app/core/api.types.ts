/**
 * The UI ↔ backend wire contract (JSON). Most types are `pitcrew_core` types passed through unchanged by the Tauri
 * commands (`app/src-tauri/src/commands.rs`), so they mirror the Rust structs exactly:
 *
 * - protocol and status types (`pitcrew_core::protocol`, `health`, `jobs`, `history`, `housekeeping`, `notify`,
 *   `metrics`, `gitea_api`) are camelCase;
 * - the configuration (`pitcrew_core::config::Config` and the policies inside it) is the `pitcrew.toml` schema
 *   verbatim, so its keys are snake_case.
 *
 * App-only types (`AppUpdateStatus`, `CommandError`, `ConnectionState`, …) mirror `app/src-tauri/src/dto.rs`.
 * Change both sides in the same commit.
 */

/** Milliseconds since the Unix epoch. */
export type Timestamp = number;
/** A semantic version without the `v`, e.g. `5.2.0`. */
export type Version = string;

/** Which runner implementation. */
export type RunnerKind = 'gitea-runner' | 'forgejo-runner';

/** The one-word status of a runner (`pitcrew_core::health::RunnerStatus`). */
export type RunnerStatus =
  | 'stopped'
  | 'paused'
  | 'waitingForLogon'
  | 'draining'
  | 'notRegistered'
  | 'crashLoop'
  | 'updating'
  | 'busy'
  | 'stalled'
  | 'starting'
  | 'degraded'
  | 'idle';

/** What waits for the current job to end (`controller::Pending`). */
export type Pending = 'stop' | 'pause' | 'restart' | 'relocate';

/** Where a runner runs (`session::SessionState`). */
export type SessionState =
  | { kind: 'notRunning' }
  | { kind: 'userSession' }
  | { kind: 'serviceAccount'; fallback: boolean }
  | { kind: 'waitingForLogon' }
  | { kind: 'unavailable'; reason: string };

/** What the runner logged about a job (`jobs::JobInfo`). `repository` may contain `***` (masked by the runner). */
export interface JobInfo {
  repository: string | null;
  job: string | null;
  jobId: number | null;
  runId: number | null;
  runNumber: number | null;
  taskId: number | null;
  server: string | null;
}

export interface CurrentJob {
  info: JobInfo;
  startedAt: Timestamp;
}

export interface FinishedJob {
  info: JobInfo;
  startedAt: Timestamp;
  finishedAt: Timestamp;
  /** How the server says it ended; present only with a Gitea read token (`gitea_api::Conclusion`). */
  outcome?: JobConclusion;
}

/** `gitea_api::Conclusion`. */
export type JobConclusion = 'success' | 'failure' | 'cancelled' | 'skipped' | 'unknown';

export interface UpdatePlan {
  from: Version | null;
  to: Version;
}

/** `update::UpdatePhase` (tag `phase`). */
export type UpdatePhase =
  | { phase: 'idle' }
  | { phase: 'downloading'; plan: UpdatePlan }
  | { phase: 'verifying'; plan: UpdatePlan }
  | { phase: 'draining'; plan: UpdatePlan; since: Timestamp }
  | { phase: 'stopping'; plan: UpdatePlan; since: Timestamp }
  | { phase: 'swapping'; plan: UpdatePlan }
  | { phase: 'confirming'; plan: UpdatePlan; since: Timestamp }
  | { phase: 'rollingBack'; plan: UpdatePlan; reason: RollbackReason };

export type RollbackReason = 'exitedEarly' | 'notAcceptedInTime';
export type PostponeReason = 'busy' | 'didNotStop';

/** `update::UpdateOutcome` (tag `result`). */
export type UpdateOutcome =
  | { result: 'updated'; plan: UpdatePlan }
  | { result: 'rolledBack'; plan: UpdatePlan; reason: RollbackReason }
  | { result: 'postponed'; plan: UpdatePlan; reason: PostponeReason }
  | { result: 'cancelled'; plan: UpdatePlan }
  | { result: 'failed'; plan: UpdatePlan; reason: string; needsAttention: boolean };

export interface UpdateRecord {
  outcome: UpdateOutcome;
  finishedAt: Timestamp;
}

export type Weekday = 'mon' | 'tue' | 'wed' | 'thu' | 'fri' | 'sat' | 'sun';

/** `update::UpdateWindow` (config schema). Times are `HH:MM`; an end before the start spans midnight. */
export interface UpdateWindow {
  days: Weekday[];
  start: string;
  end: string;
}

export type UpdateMode = 'automatic' | 'notify-only' | 'manual';
export type Channel = 'stable' | 'prerelease';

/** `update::UpdatePolicy` (config schema: snake_case). */
export interface UpdatePolicy {
  mode: UpdateMode;
  pinned: Version | null;
  window: UpdateWindow | null;
  check_every_hours: number;
  channel: Channel;
  skipped: Version[];
}

export interface DiskUse {
  folderBytes: number;
  freeBytes: number | null;
  low: boolean;
}

export type SessionMode = 'interactive' | 'service';
export type LoggedOffMode = 'wait' | 'service-account';

/** What the window shows about one runner (`protocol::RunnerSnapshot`). */
export interface RunnerSnapshot {
  name: string;
  kind: RunnerKind;
  status: RunnerStatus;
  enabled: boolean;
  pending: Pending | null;
  session: SessionState;
  runningSince: Timestamp | null;
  nextStartAt: Timestamp | null;
  restarts24h: number;
  problem: string | null;
  currentJob: CurrentJob | null;
  currentVersion: Version | null;
  latestVersion: Version | null;
  checkedAt: Timestamp | null;
  /** The version the policy would install now or in its window. */
  updateTarget: Version | null;
  lastUpdate: UpdateRecord | null;
  updatePhase: UpdatePhase;
  updatePolicy: UpdatePolicy;
  managedBinary: boolean;
  dir: string;
  labels: string[];
  serverUrl: string | null;
  sessionMode: SessionMode;
  disk: DiskUse | null;
  nextCleanupAt: Timestamp | null;
  /** Use now by the runner and its processes; `null` while it is not running. */
  usage: Usage | null;
}

/**
 * What a runner and every process it started used (`usage::Usage`). CPU is per mille of the whole machine's
 * capacity; as a sample or bucket the values are averages with the peaks next to them. ADR 0007.
 */
export interface Usage {
  cpuPermille: number;
  cpuPeakPermille: number;
  memBytes: number;
  memPeakBytes: number;
  /** Most processes counted at one moment. */
  processes: number;
}

/** One runner's use at a time (`metrics::UsagePoint`). */
export interface UsagePoint extends Usage {
  at: Timestamp;
}

/** One machine sample or aggregated point (`metrics::MachineSample`). */
export interface MachineSample {
  at: Timestamp;
  cpuPermille: number;
  cpuPeakPermille: number;
  memUsedBytes: number;
  memTotalBytes: number;
  diskFreeBytes: number;
  diskTotalBytes: number;
  /** All runners together: their processes and everything those started. Older history has zeros. */
  runners: Usage;
}

/** About the service (`protocol::ServiceInfo`). */
export interface ServiceInfo {
  version: string;
  startedAt: Timestamp;
  dataDir: string;
  configProblems: string[];
  readOnly: boolean;
  machine: MachineSample | null;
  ownCpuPermille: number;
  ownMemBytes: number;
}

export interface StatusReply {
  runners: RunnerSnapshot[];
  service: ServiceInfo;
}

export type NotificationKind =
  | 'stalled'
  | 'crashLoop'
  | 'serverUnreachable'
  | 'notRegistered'
  | 'updated'
  | 'rolledBack'
  | 'updateFailed'
  | 'updateAvailable'
  | 'diskLow'
  | 'cleanedUp'
  | 'cleanupProblems';

export type Severity = 'info' | 'warning' | 'error';

/** `notify::Notification`. */
export interface PitcrewNotification {
  kind: NotificationKind;
  severity: Severity;
  runner: string;
  title: string;
  body: string;
  at: Timestamp;
}

/** One log line with its sequence number. */
export interface LogLine {
  seq: number;
  text: string;
}

export type CleanupTrigger = 'schedule' | 'lowDisk' | 'folderCap' | 'manual';

/** `history::HistoryEvent` (tag `kind`). */
export type HistoryEvent = { at: Timestamp } & (
  | { kind: 'started' }
  | { kind: 'exited'; code: number | null; expected: boolean }
  | { kind: 'stalled'; timeouts: number }
  | { kind: 'crashLoop'; exits: number }
  | { kind: 'launchFailed'; reason: string }
  | { kind: 'stopped' }
  | { kind: 'paused' }
  | { kind: 'resumed' }
  | { kind: 'jobFinished'; job: FinishedJob }
  | { kind: 'updateFinished'; outcome: UpdateOutcome }
  | { kind: 'cleanup'; trigger: CleanupTrigger; freedBytes: number; deleted: number; failed: number }
);

export interface Span {
  from: Timestamp;
  to: Timestamp;
}

export type MarkKind =
  | 'crash'
  | 'stall'
  | 'crashLoop'
  | 'launchFailed'
  | 'updated'
  | 'rolledBack'
  | 'updateFailed'
  | 'updateSkipped'
  | 'cleanup';

export interface Mark {
  at: Timestamp;
  kind: MarkKind;
}

export interface DaySummary {
  start: Timestamp;
  jobs: number;
  restarts: number;
  uptimePermille: number;
  busyMs: number;
  avgJobMs: number;
}

/** `history::HistorySummary`. */
export interface HistorySummary {
  from: Timestamp;
  to: Timestamp;
  days: DaySummary[];
  uptimePermille: number;
  restarts: number;
  jobs: number;
  up: Span[];
  busy: Span[];
  marks: Mark[];
  updates: { at: Timestamp; outcome: UpdateOutcome }[];
  recentJobs: FinishedJob[];
}

/** Jobs per day by outcome (`gitea_api::JobDay`); outcomes and queue times need a Gitea read token. */
export interface JobDay {
  start: Timestamp;
  success: number;
  failure: number;
  cancelled: number;
  unknown: number;
  avgQueuedMs: number | null;
}

/** The Activity data of one runner: the history summary, job outcomes per day, and the recent events. */
export interface ActivityReply {
  summary: HistorySummary;
  jobDays: JobDay[];
  /** The same by local hour over the last 24 hours. */
  jobHours: JobDay[];
  /** The newest events, newest first (at most 200), for the events feed. */
  events: HistoryEvent[];
  /** Whether a Gitea read token is set for the runner's server. */
  hasApiToken: boolean;
}

/** Machine metrics and a runner folder's size over a range. */
export interface MetricsReply {
  machine: MachineSample[];
  /** `[time, bytes]` pairs. */
  folder: [Timestamp, number][];
  /** The requested runner's own CPU and memory; empty without a runner. */
  usage: UsagePoint[];
}

/** `housekeeping::EntryKind`. */
export type EntryKind = 'work' | 'cache' | 'download' | 'previous' | 'oldBinary' | { rotatedLog: { n: number } };

export interface DiskEntry {
  path: string;
  kind: EntryKind;
  bytes: number;
  modified: Timestamp;
}

export interface DiskScan {
  entries: DiskEntry[];
  folderBytes: number;
  freeBytes: number | null;
  truncated: boolean;
}

export type DeleteReason = 'leftover' | 'oldLog' | 'aged' | 'cacheCap' | 'pressure';

export interface CleanupPlan {
  trigger: CleanupTrigger;
  deletions: { entry: DiskEntry; reason: DeleteReason }[];
  freedBytes: number;
  stillNeededBytes: number;
}

export interface CleanupReport {
  trigger: CleanupTrigger;
  at: Timestamp;
  freedBytes: number;
  deleted: number;
  failures: { path: string; reason: string }[];
}

/** `housekeeping::CleanupPolicy` (config schema: snake_case). */
export interface CleanupPolicy {
  automatic: boolean;
  every_hours: number;
  work_max_age_hours: number;
  cache_cap_mb: number;
  folder_cap_mb: number;
  min_free_mb: number;
  keep_previous: boolean;
}

/** The disk page's data: the scan, the dry-run preview of a manual cleanup, the last cleanup and the policy. */
export interface DiskReply {
  scan: DiskScan;
  preview: CleanupPlan;
  lastCleanup: CleanupReport | null;
  policy: CleanupPolicy;
}

/** The current job's details from the Gitea API (`gitea_api::RunDetails`). */
export interface RunDetails {
  title: string | null;
  workflow: string | null;
  branch: string | null;
  event: string | null;
  url: string | null;
}

export interface StallRule {
  threshold: number;
  window_secs: number;
}

export interface RestartPolicy {
  initial_delay_secs: number;
  max_delay_secs: number;
  reset_after_secs: number;
  crash_loop_restarts: number;
  crash_loop_window_secs: number;
  crash_loop_cooldown_secs: number;
}

/** One runner in `pitcrew.toml` (snake_case). */
export interface RunnerConfig {
  name: string;
  kind: RunnerKind;
  dir: string;
  config_file?: string | null;
  labels: string[];
  session: SessionMode;
  logged_off: LoggedOffMode;
  server_url?: string | null;
  enabled: boolean;
  binary_path?: string | null;
  update?: UpdatePolicy | null;
  watchdog?: StallRule | null;
  cleanup?: CleanupPolicy | null;
}

/** `pitcrew.toml` (`pitcrew_core::config::Config`, snake_case). */
export interface Config {
  schema_version: number;
  defaults: {
    update: UpdatePolicy;
    update_timeouts: { drain_secs: number; stop_secs: number; confirm_secs: number };
    watchdog: StallRule;
    restart: RestartPolicy;
    logs: { max_bytes: number; keep: number };
    cleanup: CleanupPolicy;
  };
  runners: RunnerConfig[];
}

export interface SettingsReply {
  config: Config;
  problems: string[];
  readOnly: boolean;
}

/** The registration wizard's request. The token goes to the service and the runner's `register` only. */
export interface RegisterRequest {
  /** Pitcrew's name for the runner (a-z, 0-9, '-'). */
  runner: string;
  serverUrl: string;
  token: string;
  /** The name the server shows; defaults to `runner`. */
  displayName: string | null;
  labels: string[];
  /** The runner's folder; defaults to `<data folder>/runners/<runner>`. An existing registered folder is adopted. */
  dir: string | null;
  session: SessionMode;
  loggedOff: LoggedOffMode;
  /** Keep the token in the OS credential store (Windows Credential Manager / macOS Keychain) for re-registering. */
  rememberToken: boolean;
}

/** Whether Pitcrew can update itself (Tauri updater). */
export interface AppUpdateStatus {
  /** False in builds without the update public key. */
  available: boolean;
  /** Why not, for the user. */
  reason: string | null;
  currentVersion: string;
}

/** A Pitcrew release found by a check. */
export interface AppUpdateInfo {
  version: string;
  notes: string | null;
  date: string | null;
}

/** The app's connection to the service. */
export type ConnectionState =
  | { state: 'connecting' }
  | { state: 'connected'; serviceVersion: string }
  | { state: 'unavailable'; message: string }
  | { state: 'mismatch'; message: string };

/** The service's registration with the OS. */
export interface ServiceInstall {
  platform: 'windows' | 'macos' | 'linux' | 'browser';
  installed: boolean;
  running: boolean;
  /** Installing asks for the OS's own permission (UAC on Windows, Login Items approval on macOS). */
  canInstall: boolean;
  message: string | null;
}

/** Something the backend pushes. */
export type BackendEvent =
  | { kind: 'status'; status: StatusReply }
  | { kind: 'notification'; notification: PitcrewNotification }
  | { kind: 'connection'; connection: ConnectionState };

/** A command error (`app/src-tauri/src/dto.rs` `CommandError`). */
export interface CommandError {
  code:
    | 'notImplemented'
    | 'serviceUnavailable'
    | 'protocolMismatch'
    | 'unknownRunner'
    | 'badRequest'
    | 'permissionDenied'
    | 'busy'
    | 'cancelled'
    | 'internal'
    /** Raised by the UI, not the shell: a call did not answer in time (`call-timeout.ts`). */
    | 'timeout';
  message: string;
}
