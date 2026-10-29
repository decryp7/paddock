import type {
  ActivityKind,
  AlertKind,
  AnomalyKind,
  CauseKind,
  Forecast,
  Grade,
  IssueKind,
  IssueSourceId,
  ImpactRating,
  MemoryPressure,
  Priority,
  Reading,
  Severity,
  SignatureInfo,
  StartupLocation,
  StartupState,
  TimelineEventKind,
} from '../core/api.types';
import type { IconName } from './icons';

/** Words and tones for the engine's enums. Presentation only: no thresholds or rules live here. */
export type Tone = 'ok' | 'warn' | 'bad' | 'info' | 'accent' | 'muted';

export const PRIORITY_LABEL: Record<Priority, string> = {
  idle: 'Low',
  belowNormal: 'Below normal',
  normal: 'Normal',
  aboveNormal: 'Above normal',
  high: 'High',
};

export const CAUSE_ICON: Record<CauseKind, IconName> = {
  nothingWrong: 'check',
  cpuHog: 'cpu',
  memoryPressure: 'memory',
  diskQueue: 'disk',
  thermalThrottling: 'temperature',
  osUpdate: 'refresh',
  antivirusScan: 'shield',
  indexing: 'search',
  lowDiskSpace: 'disk',
  gpuBusy: 'gpu',
};

export const ANOMALY_LABEL: Record<AnomalyKind, string> = {
  memoryGrowth: 'Memory growth',
  periodicSpike: 'Periodic spikes',
  restartLoop: 'Restart loop',
};

export const ANOMALY_ICON: Record<AnomalyKind, IconName> = {
  memoryGrowth: 'trend',
  periodicSpike: 'bolt',
  restartLoop: 'refresh',
};

export const EVENT_LABEL: Record<TimelineEventKind, string> = {
  alert: 'Alert',
  boot: 'Restart',
  network: 'Network',
};

export const ACTIVITY_ICON: Record<ActivityKind, IconName> = {
  alert: 'bell',
  system: 'info',
};

export const ACTIVITY_LABEL: Record<ActivityKind, string> = {
  alert: 'Alert',
  system: 'Dyno',
};

export const GRADE_LABEL: Record<Grade, string> = { good: 'Good', fair: 'Fair', poor: 'Poor' };
export const GRADE_TONE: Record<Grade, 'ok' | 'warn' | 'bad' | 'accent'> = { good: 'ok', fair: 'warn', poor: 'bad' };

export const SEVERITY_LABEL: Record<Severity, string> = { info: 'Info', warning: 'Warning', critical: 'Critical' };
export const SEVERITY_TONE: Record<Severity, Tone> = { info: 'info', warning: 'warn', critical: 'bad' };

export const PRESSURE_LABEL: Record<MemoryPressure, string> = { normal: 'Normal', elevated: 'Elevated', critical: 'Critical' };
export const PRESSURE_TONE: Record<MemoryPressure, Tone> = { normal: 'ok', elevated: 'warn', critical: 'bad' };

export const LOCATION_LABEL: Record<StartupLocation, string> = {
  userRunKey: 'Registry: Run (you)',
  machineRunKey: 'Registry: Run (all users)',
  userStartupFolder: 'Startup folder (you)',
  commonStartupFolder: 'Startup folder (all users)',
  scheduledTask: 'Scheduled task',
  userLaunchAgent: 'Launch agent (you)',
  systemLaunchAgent: 'Launch agent (system)',
  loginItem: 'Login item',
};

export const IMPACT_LABEL: Record<ImpactRating, string> = { notMeasured: 'Not measured', low: 'Low', medium: 'Medium', high: 'High' };
export const IMPACT_TONE: Record<ImpactRating, Tone> = { notMeasured: 'muted', low: 'ok', medium: 'warn', high: 'bad' };

export const ALERT_ICON: Record<AlertKind, IconName> = {
  lowDisk: 'disk',
  overheating: 'temperature',
  memoryPressure: 'memory',
  crashLoop: 'refresh',
  longHighCpu: 'cpu',
};

export const ALERT_LABEL: Record<AlertKind, string> = {
  lowDisk: 'Low disk space',
  overheating: 'Overheating',
  memoryPressure: 'Memory pressure',
  crashLoop: 'Crash loop',
  longHighCpu: 'Long high CPU',
};

export function startupStateText(state: StartupState): string {
  return state === 'enabled' ? 'Enabled' : 'Disabled';
}

export function signatureText(sig: SignatureInfo): { text: string; tone: Tone; verified: boolean } {
  if (sig === 'unsigned') return { text: 'Not signed', tone: 'warn', verified: false };
  if ('signed' in sig) return { text: sig.signed, tone: 'ok', verified: true };
  if ('invalid' in sig) return { text: `Invalid signature: ${sig.invalid}`, tone: 'bad', verified: false };
  return { text: sig.unknown, tone: 'muted', verified: false };
}

export function forecastText(forecast: Forecast): string {
  if (forecast === 'notFilling') return 'Not filling up';
  if (forecast === 'notEnoughData') return 'Not enough history yet';
  return `Full in ~${Math.round(forecast.fullInDays)} days`;
}

/** The value of a reading, or `null` when the platform doesn't provide it. */
export function readingValue<T>(r: Reading<T>): T | null {
  return 'available' in r ? r.available : null;
}

/** Why a reading is missing, or `null` when it is available. */
export function readingReason<T>(r: Reading<T>): string | null {
  return 'unavailable' in r ? r.unavailable : null;
}

/** The engine's names for what went wrong (`IssueKind::label`). */
export const ISSUE_KIND_LABEL: Record<IssueKind, string> = {
  appCrash: 'App crash',
  appHang: 'App not responding',
  badShutdown: 'Unexpected shutdown',
  diskError: 'Disk error',
  memoryError: 'Memory error',
  overheating: 'Overheating',
  serviceFailure: 'Service failure',
  memoryExhaustion: 'Out of memory',
  updateFailure: 'Update failure',
  driverFailure: 'Driver failure',
};

/** Every issue kind, in the order the engine lists them. */
export const ISSUE_KINDS = Object.keys(ISSUE_KIND_LABEL) as IssueKind[];

/** Where an issue was read from (`IssueSourceId::label`). */
export const ISSUE_SOURCE_LABEL: Record<IssueSourceId, string> = {
  macReports: 'Crash reports',
  macLog: 'System log',
  windowsEvents: 'Windows Event Log',
  journal: 'System journal',
  coredumps: 'Core dumps',
};
