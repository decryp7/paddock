import type { Schedule, ScheduleMode, Weekday } from '../core/api.types';

/** The presets of the "Every" mode. */
export const EVERY_PRESETS: { secs: number; label: string }[] = [
  { secs: 900, label: '15 min' },
  { secs: 1800, label: '30 min' },
  { secs: 3600, label: '1 h' },
  { secs: 3 * 3600, label: '3 h' },
  { secs: 6 * 3600, label: '6 h' },
  { secs: 12 * 3600, label: '12 h' },
  { secs: 86_400, label: 'Daily' },
];

export type IntervalUnit = 'minutes' | 'hours' | 'days';

export const INTERVAL_UNITS: { id: IntervalUnit; label: string; secs: number }[] = [
  { id: 'minutes', label: 'Minutes', secs: 60 },
  { id: 'hours', label: 'Hours', secs: 3600 },
  { id: 'days', label: 'Days', secs: 86_400 },
];

export const SCHEDULE_MODES: { id: ScheduleMode; label: string }[] = [
  { id: 'every', label: 'Every…' },
  { id: 'times', label: 'At times' },
  { id: 'cron', label: 'Cron' },
];

export const WEEKDAYS: { id: Weekday; label: string; full: string }[] = [
  { id: 'mon', label: 'Mon', full: 'Monday' },
  { id: 'tue', label: 'Tue', full: 'Tuesday' },
  { id: 'wed', label: 'Wed', full: 'Wednesday' },
  { id: 'thu', label: 'Thu', full: 'Thursday' },
  { id: 'fri', label: 'Fri', full: 'Friday' },
  { id: 'sat', label: 'Sat', full: 'Saturday' },
  { id: 'sun', label: 'Sun', full: 'Sunday' },
];

/** An interval as a number and the largest unit that divides it evenly (minutes when none does, rounded up). */
export function splitInterval(secs: number): { amount: number; unit: IntervalUnit } {
  const unit = [...INTERVAL_UNITS].reverse().find((u) => secs >= u.secs && secs % u.secs === 0) ?? INTERVAL_UNITS[0]!;
  return { amount: Math.max(1, Math.ceil(secs / unit.secs)), unit: unit.id };
}

/** Seconds for a number and unit, or `null` when the number is not a positive whole number. */
export function intervalSecs(amount: number, unit: IntervalUnit): number | null {
  if (!Number.isFinite(amount) || amount < 1) return null;
  const secs = Math.round(amount) * INTERVAL_UNITS.find((u) => u.id === unit)!.secs;
  return secs <= 0xffffffff ? secs : null;
}

/** The schedule to show after switching mode: the kept settings when switching back, else a sensible start. */
export function schedulePerMode(mode: ScheduleMode, current: Schedule, everySecs: number): Schedule {
  if (current.mode === mode) return current;
  const zone = current.mode === 'every' ? null : current.zone;
  if (mode === 'every') return { mode, secs: everySecs };
  if (mode === 'times') return { mode, days: ['mon', 'tue', 'wed', 'thu', 'fri'], times: ['09:00'], zone };
  return { mode, expr: '0 9 * * 1-5', zone };
}

/** Ticks or unticks a day; the result stays in week order. */
export function toggleDay(days: Weekday[], day: Weekday): Weekday[] {
  const on = days.includes(day) ? days.filter((d) => d !== day) : [...days, day];
  return WEEKDAYS.map((d) => d.id).filter((d) => on.includes(d));
}

/** Adds a time of day after the last one (an hour later, wrapping at midnight). */
export function addTime(times: string[]): string[] {
  const last = [...times].sort().pop();
  const hour = last ? (Number(last.slice(0, 2)) + 1) % 24 : 9;
  return [...times, `${String(hour).padStart(2, '0')}:00`];
}

export function setTime(times: string[], index: number, value: string): string[] {
  return times.map((t, i) => (i === index ? value : t));
}

export function removeTime(times: string[], index: number): string[] {
  return times.filter((_, i) => i !== index);
}

/** The zone text as stored: trimmed, and `null` when empty (the computer's own zone). */
export function zoneValue(text: string): string | null {
  return text.trim() === '' ? null : text.trim();
}
