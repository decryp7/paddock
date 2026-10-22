import type {
  ActivityKind,
  AlertKind,
  AnomalyKind,
  CauseKind,
  CleanupGroup,
  CleanupRisk,
  Forecast,
  Grade,
  ImpactRating,
  MemoryPressure,
  Priority,
  ProcessAction,
  ProfileId,
  Reading,
  RuleContext,
  RuleTarget,
  Severity,
  SignatureInfo,
  StartupLocation,
  StartupState,
  StepAction,
  TimelineEventKind,
  UndoState,
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

export const PRIORITIES: Priority[] = ['idle', 'belowNormal', 'normal', 'aboveNormal', 'high'];

export const PROFILE_ICON: Record<ProfileId, IconName> = {
  normal: 'profile-normal',
  gaming: 'profile-gaming',
  meeting: 'profile-meeting',
  onBattery: 'profile-onBattery',
  buildingCode: 'profile-buildingCode',
};

export const PROFILE_ORDER: ProfileId[] = ['normal', 'gaming', 'meeting', 'onBattery', 'buildingCode'];

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
  throttle: 'Rule: throttle',
  cap: 'Rule: CPU cap',
  release: 'Rule: release',
  cleanup: 'Cleanup',
  tuning: 'Tuning',
  alert: 'Alert',
  boot: 'Restart',
};

export const ACTIVITY_ICON: Record<ActivityKind, IconName> = {
  rule: 'rules',
  release: 'play',
  manual: 'processes',
  cleanup: 'cleanup',
  tuning: 'sparkle',
  startup: 'power',
  alert: 'bell',
  system: 'info',
};

export const ACTIVITY_LABEL: Record<ActivityKind, string> = {
  rule: 'Rule',
  release: 'Released',
  manual: 'You',
  cleanup: 'Cleanup',
  tuning: 'Tuning',
  startup: 'Startup',
  alert: 'Alert',
  system: 'Dyno',
};

export const UNDO_LABEL: Record<UndoState, string> = {
  notUndoable: '',
  available: 'Undo',
  undone: 'Undone',
  expired: 'Expired',
};

export const GROUP_LABEL: Record<CleanupGroup, string> = {
  system: 'System',
  browser: 'Browsers',
  developer: 'Developer',
  downloads: 'Downloads',
  docker: 'Docker',
  plugin: 'Plugins',
};

export const GROUP_ICON: Record<CleanupGroup, IconName> = {
  system: 'monitor',
  browser: 'globe',
  developer: 'profile-buildingCode',
  downloads: 'arrow-down',
  docker: 'layers',
  plugin: 'puzzle',
};

export const GROUP_ORDER: CleanupGroup[] = ['system', 'browser', 'developer', 'downloads', 'docker', 'plugin'];

export const RISK_LABEL: Record<CleanupRisk, string> = { safe: 'Safe', review: 'Review', personal: 'Personal files' };
export const RISK_TONE: Record<CleanupRisk, Tone> = { safe: 'ok', review: 'warn', personal: 'bad' };

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

export const CONTEXT_LABEL: Record<RuleContext, string> = {
  userActive: 'while you are active',
  userIdle: 'while you are away',
  onBattery: 'on battery',
  onAc: 'plugged in',
};

export function startupStateText(state: StartupState): string {
  if (state === 'enabled') return 'Enabled';
  if (state === 'disabled') return 'Disabled';
  return `Delayed ${state.delayed} s`;
}

export function stepText(action: StepAction): string {
  if (action === 'lowerPriority') return 'Lower priority';
  if (action === 'efficiencyMode') return 'Efficiency mode';
  if (action === 'notify') return 'Notify me';
  return `Cap at ${action.cpuCap}% CPU`;
}

export function targetText(target: RuleTarget): string {
  if (target === 'anyApp') return 'Any app';
  if (target.apps.length <= 2) return target.apps.join(', ');
  return `${target.apps.slice(0, 2).join(', ')} +${target.apps.length - 2}`;
}

export function actionText(action: ProcessAction): string {
  if (action === 'end') return 'End';
  if (action === 'suspend') return 'Suspend';
  if (action === 'resume') return 'Resume';
  if ('setPriority' in action) return `Priority: ${PRIORITY_LABEL[action.setPriority]}`;
  if ('setAffinity' in action) return 'Change cores';
  if ('setEfficiencyMode' in action) return action.setEfficiencyMode ? 'Efficiency mode on' : 'Efficiency mode off';
  return action.setCpuCap === null ? 'Remove CPU cap' : `Cap at ${action.setCpuCap}% CPU`;
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
