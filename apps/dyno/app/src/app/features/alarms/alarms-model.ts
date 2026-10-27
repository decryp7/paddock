import type { Alarm, AlarmChange, AlarmConfig, AlarmQuietHours, AlarmState, CustomAlertRule, CustomMetric, Severity, Shelf } from '../../core/api.types';
import { formatClock, formatDate } from '../../shared/format';

// The open-list helpers live with the state (the store uses them); re-exported for the page code and its specs.
export { mergeAlarms, needsAcknowledge, prependChange, sortOpen } from '../../state/alarm-list';

/** The metrics a custom rule can watch, in the order the dialog lists them. */
export const CUSTOM_METRICS: readonly { id: CustomMetric; label: string; unit: '%' | '°C'; hint: string }[] = [
  { id: 'cpuTotal', label: 'CPU', unit: '%', hint: 'Total processor load' },
  { id: 'memoryUsed', label: 'Memory', unit: '%', hint: 'Memory in use' },
  { id: 'memoryPressure', label: 'Memory pressure', unit: '%', hint: 'How hard the system works to free memory' },
  { id: 'swap', label: 'Swap', unit: '%', hint: 'Swap or page file in use' },
  { id: 'diskFree', label: 'Free disk space', unit: '%', hint: 'Free space on the system disk' },
  { id: 'gpuLoad', label: 'GPU', unit: '%', hint: 'Graphics processor load' },
  { id: 'temperature', label: 'Temperature', unit: '°C', hint: 'The hottest sensor' },
  { id: 'battery', label: 'Battery', unit: '%', hint: 'Battery charge' },
];

export function metricInfo(id: CustomMetric) {
  return CUSTOM_METRICS.find((m) => m.id === id) ?? CUSTOM_METRICS[0]!;
}

/** `45 sec`, `2 min`, `1 min 30 sec`; 0 means the condition alone is enough. */
export function formatHold(secs: number): string {
  if (secs < 60) return `${secs} sec`;
  const m = Math.floor(secs / 60);
  return secs % 60 === 0 ? `${m} min` : `${m} min ${secs % 60} sec`;
}

/** "CPU is above 90% for 2 min". */
export function ruleSummary(r: CustomAlertRule): string {
  const m = metricInfo(r.metric);
  const base = `${m.label} is ${r.op} ${r.limit}${m.unit === '%' ? '%' : ' °C'}`;
  return r.forSecs > 0 ? `${base} for ${formatHold(r.forSecs)}` : base;
}

/** A URL-safe id from a rule name that none of `taken` uses (`gpu-limit`, `gpu-limit-2`, ...). */
export function uniqueRuleId(name: string, taken: readonly string[]): string {
  const slug =
    name
      .toLowerCase()
      .normalize('NFKD')
      .replace(/[\u0300-\u036f]/g, '')
      .replace(/[^a-z0-9]+/g, '-')
      .replace(/^-+|-+$/g, '')
      .slice(0, 40)
      .replace(/-+$/, '') || 'rule';
  if (!taken.includes(slug)) return slug;
  for (let n = 2; ; n++) {
    const id = `${slug}-${n}`;
    if (!taken.includes(id)) return id;
  }
}

/** `HH:MM` for minutes after midnight. */
export function minutesToClock(minute: number): string {
  const m = Math.min(1439, Math.max(0, Math.round(minute)));
  return `${String(Math.floor(m / 60)).padStart(2, '0')}:${String(m % 60).padStart(2, '0')}`;
}

/** Minutes after midnight for `HH:MM`, or null when the text is not a time. */
export function clockToMinutes(text: string): number | null {
  const m = /^(\d{1,2}):(\d{2})$/.exec(text.trim());
  if (!m) return null;
  const h = Number(m[1]);
  const min = Number(m[2]);
  return h < 24 && min < 60 ? h * 60 + min : null;
}

/** The engine's default when settings carry no alarm configuration yet. */
export function defaultAlarmConfig(utcOffsetMinutes: number): AlarmConfig {
  return {
    quiet: { enabled: false, startMinute: 22 * 60, endMinute: 7 * 60, criticalBypass: true },
    chatterTransitions: 6,
    chatterWindowMinutes: 30,
    maxAlarms: 500,
    keepClearedDays: 14,
    utcOffsetMinutes,
  };
}

export function withQuiet(config: AlarmConfig, patch: Partial<AlarmQuietHours>): AlarmConfig {
  return { ...config, quiet: { ...config.quiet, ...patch } };
}

export function severityWord(s: Severity): string {
  return s === 'critical' ? 'Critical' : s === 'warning' ? 'Warning' : 'Info';
}

export function stateWord(s: AlarmState): string {
  switch (s) {
    case 'active':
      return 'Active';
    case 'acknowledged':
      return 'Acknowledged';
    case 'clearedUnacknowledged':
      return 'Returned, not acknowledged';
    case 'cleared':
      return 'Cleared';
  }
}

/** The tone of a state's dot: attention for the unacknowledged, calm otherwise. */
export function stateTone(s: AlarmState): 'bad' | 'warn' | 'calm' {
  return s === 'active' ? 'bad' : s === 'clearedUnacknowledged' ? 'warn' : 'calm';
}

export function isShelved(shelf: Shelf | null, now: number): boolean {
  return shelf === 'untilReboot' || (shelf !== null && shelf.until > now);
}

/** Time of day for today, date and time for other days: `14:05`, `22 Oct, 14:05`. */
export function shortTime(at: number, now: number): string {
  const same = new Date(at).toDateString() === new Date(now).toDateString();
  return same ? formatClock(at) : `${formatDate(at, now)}, ${formatClock(at)}`;
}

/** The single muted suffix for an alarm's side states ("Shelved until 14:05 · Muted"), or an empty string. */
export function alarmSuffix(a: Alarm, now: number): string {
  const parts: string[] = [];
  if (isShelved(a.shelved, now)) parts.push(a.shelved === 'untilReboot' ? 'Shelved until restart' : `Shelved until ${shortTime((a.shelved as { until: number }).until, now)}`);
  if (a.muted) parts.push('Muted');
  if (a.chattering) parts.push('Chattering');
  return parts.join(' · ');
}

export function changeLabel(c: AlarmChange): string {
  switch (c.kind) {
    case 'raised':
      return 'Raised';
    case 'repeated':
      return 'Repeated';
    case 'returned':
      return 'Returned to normal';
    case 'acknowledged':
      return 'Acknowledged';
    case 'shelved':
      return 'Shelved';
    case 'unshelved':
      return 'Unshelved';
    case 'muted':
      return c.alarm.muted ? 'Rule muted' : 'Rule unmuted';
    case 'noted':
      return 'Note added';
    case 'chattering':
      return 'Started chattering';
    case 'pruned':
      return 'Removed';
  }
}

