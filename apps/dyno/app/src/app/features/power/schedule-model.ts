import type { DynoJob, WakeInput, WakeRepeat, WakeRow, WakeRowState, WakeRouteState, WakeSchedule } from '../../core/api.types';
import { formatDayTime } from '../../shared/format';

export const WEEKDAYS = ['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun'] as const;
export const JOB_LABELS: Record<DynoJob, string> = { backupCheck: 'Check the backups', softwareInventory: 'List the installed software' };

/** "07:05" from minutes after midnight. */
export function clock(minute: number): string {
  return new Date(2000, 0, 1, Math.floor(minute / 60), minute % 60).toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit' });
}

/** "HH:MM" for a time input (it shows the system's own format; its value is always 24-hour). */
export function inputClock(minute: number): string {
  return `${String(Math.floor(minute / 60)).padStart(2, '0')}:${String(minute % 60).padStart(2, '0')}`;
}

/** Minutes after midnight from a time input's "HH:MM", or null. */
export function minuteOf(text: string): number | null {
  const m = /^(\d{1,2})[:.](\d{2})\s*([ap]\.?m\.?)?$/i.exec(text.trim());
  if (!m) return null;
  let h = Number(m[1]);
  const mi = Number(m[2]);
  if (m[3]) {
    // A 12-hour time: 12 am is midnight, 12 pm is noon.
    if (h < 1 || h > 12) return null;
    h = (h % 12) + (m[3].toLowerCase().startsWith('p') ? 12 : 0);
  }
  return h < 24 && mi < 60 ? h * 60 + mi : null;
}

/** When it repeats, in words. */
export function repeatText(r: WakeRepeat): string {
  if (r === 'once') return 'Once';
  if (r === 'daily') return 'Every day';
  const mask = r.days.mask;
  if (mask === 0b0011111) return 'Weekdays';
  if (mask === 0b1100000) return 'Weekends';
  return WEEKDAYS.filter((_, i) => mask & (1 << i)).join(', ');
}

/** What happens after the wake, in words (computers are named by the caller's lookup). */
export function thenText(s: WakeSchedule, deviceName: (id: number) => string): string {
  const parts: string[] = [];
  if (s.stayAwakeMinutes !== null) parts.push(`stay awake ${s.stayAwakeMinutes} min`);
  if (s.wakeDevices.length > 0) parts.push(`wake ${s.wakeDevices.map(deviceName).join(', ')}`);
  if (s.job) parts.push(JOB_LABELS[s.job].toLowerCase());
  const text = parts.join('; ');
  return text.charAt(0).toUpperCase() + text.slice(1);
}

export type RowKind = 'armed' | 'off' | 'paused' | 'needsHelper' | 'failed';

export function rowKind(s: WakeRowState): RowKind {
  return typeof s === 'string' ? s : 'armed' in s ? 'armed' : 'failed';
}

/** The row's status in words (never colour alone). */
export function statusText(row: WakeRow): string {
  const s = row.state;
  if (s === 'off') return 'Off';
  if (s === 'paused') return 'Paused';
  if (s === 'needsHelper') return 'Needs the helper';
  if ('failed' in s) return 'Not set';
  return row.entry.nextAt === null ? 'Armed' : `Next wake ${formatDayTime(row.entry.nextAt)}`;
}

/** Why a row's wake is not set, or null. */
export function failedWhy(row: WakeRow): string | null {
  return typeof row.state === 'object' && 'failed' in row.state ? row.state.failed.why : null;
}

/** The last run as a short phrase and whether it went well (a missed wake is not an error but is not a run either). */
export function lastRunShort(s: WakeSchedule): { text: string; ok: boolean } | null {
  if (!s.lastRun) return null;
  return { text: `${formatDayTime(s.lastRun.at)}: ${s.lastRun.summary.replace(/\.$/, '')}`, ok: !s.lastRun.summary.startsWith('Missed') };
}

/** The last run, in words. */
export function lastRunText(s: WakeSchedule): string {
  return s.lastRun ? `${s.lastRun.summary.replace(/\.$/, '')} (${formatDayTime(s.lastRun.at)})` : 'Has not run yet';
}

export type RouteKind = 'agent' | 'helperReady' | 'needsHelper' | 'unavailable';

export function routeKind(r: WakeRouteState): RouteKind {
  return typeof r === 'string' ? r : 'agent' in r ? 'agent' : 'needsHelper' in r ? 'needsHelper' : 'unavailable';
}

/** What Dyno cannot tell on this route, one line each with where to check. */
export function routeCantTell(r: WakeRouteState): string[] {
  return typeof r === 'object' && 'agent' in r ? r.agent.cantTell : [];
}

export function routeText(r: WakeRouteState): string {
  if (typeof r === 'string') return 'The administrator helper sets the wake. A computer that is shut down is not woken.';
  if ('agent' in r) return r.agent.note;
  if ('needsHelper' in r) return r.needsHelper.reason;
  return r.unavailable.reason;
}

export function blankWake(): WakeInput {
  return { id: null, name: '', minuteOfDay: 7 * 60, repeat: 'daily', stayAwakeMinutes: 30, wakeDevices: [], job: null, enabled: true };
}

export function wakeInputFrom(s: WakeSchedule): WakeInput {
  return { id: s.id, name: s.name, minuteOfDay: s.minuteOfDay, repeat: s.repeat, stayAwakeMinutes: s.stayAwakeMinutes, wakeDevices: [...s.wakeDevices], job: s.job, enabled: s.enabled };
}

/** Which field the engine's refusal is about, from its words (the engine validates; this only places them). */
export function wakeFieldOf(message: string | undefined): 'name' | 'time' | 'repeat' | 'then' | null {
  const m = (message ?? '').toLowerCase();
  if (m.includes('time of day')) return 'time';
  if (m.includes('name')) return 'name';
  if (m.includes('at least one day')) return 'repeat';
  if (m.includes('what happens') || m.includes('stay awake') || m.includes('computer')) return 'then';
  return null;
}
