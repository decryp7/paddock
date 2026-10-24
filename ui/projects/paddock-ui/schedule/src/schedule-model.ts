/**
 * The wire model of `paddock_jobs::schedule` (plan X33; camelCase JSON, same shapes as the Rust types) and the pure helpers the
 * editors use. The engine owns the rules (next run, daylight saving, catch-up, validation); this file only shapes the
 * editor's inputs and outputs.
 */

export type Weekday = 'mon' | 'tue' | 'wed' | 'thu' | 'fri' | 'sat' | 'sun';
export type ScheduleMode = 'every' | 'times' | 'cron';

/** When something runs: every so often, at times of day on chosen days, or by a cron expression. */
export type Schedule =
  | { readonly mode: 'every'; readonly secs: number }
  | { readonly mode: 'times'; readonly days: readonly Weekday[]; readonly times: readonly string[]; readonly zone: string | null }
  | { readonly mode: 'cron'; readonly expr: string; readonly zone: string | null };

/** What the engine's `preview` answers for the editor: the schedule in words, the next runs and things worth knowing. */
export interface SchedulePreview {
  readonly description: string;
  /** Milliseconds since the Unix epoch. */
  readonly nextRuns: readonly number[];
  readonly warnings: readonly string[];
  /** The IANA name (or `UTC+02:00`) the times are in. */
  readonly zone: string;
}

/** One pause window: the days it starts on (empty is every day), when it opens and closes (`HH:MM`, may span midnight). */
export interface PauseWindow {
  readonly days: readonly Weekday[];
  readonly start: string;
  readonly end: string;
}

/** A set of pause windows in one time zone (`null`: the computer's own). */
export interface PauseWindows {
  readonly windows: readonly PauseWindow[];
  readonly zone: string | null;
}

/** Most windows the engine accepts (`paddock_jobs::schedule::pause::MAX_WINDOWS`). */
export const MAX_PAUSE_WINDOWS = 14;
/** Most times of day the engine accepts (`MAX_TIMES`). */
export const MAX_TIMES = 24;

/** Week order, Monday first, with the short and full English names (the labels are localized in the components). */
export const WEEKDAYS: readonly Weekday[] = ['mon', 'tue', 'wed', 'thu', 'fri', 'sat', 'sun'];

/** The presets of the "Every" mode, in seconds. */
export const EVERY_PRESETS: readonly number[] = [900, 1800, 3600, 3 * 3600, 6 * 3600, 12 * 3600, 86_400];

export type IntervalUnit = 'minutes' | 'hours' | 'days';

export const INTERVAL_UNITS: readonly { readonly id: IntervalUnit; readonly secs: number }[] = [
  { id: 'minutes', secs: 60 },
  { id: 'hours', secs: 3600 },
  { id: 'days', secs: 86_400 },
];

/** An interval as a number and the largest unit that divides it evenly (minutes when none does, rounded up). */
export function splitInterval(secs: number): { amount: number; unit: IntervalUnit } {
  const unit = [...INTERVAL_UNITS].reverse().find((u) => secs >= u.secs && secs % u.secs === 0) ?? INTERVAL_UNITS[0]!;
  return { amount: Math.max(1, Math.ceil(secs / unit.secs)), unit: unit.id };
}

/** Seconds for a number and unit, or `null` when the number is not a positive number the wire can hold. */
export function intervalSecs(amount: number, unit: IntervalUnit): number | null {
  if (!Number.isFinite(amount) || amount < 1) return null;
  const secs = Math.round(amount) * INTERVAL_UNITS.find((u) => u.id === unit)!.secs;
  return secs <= 0xffffffff ? secs : null;
}

/** The schedule to show after switching mode: the current one when unchanged, else a sensible start (zone carried over). */
export function scheduleForMode(mode: ScheduleMode, current: Schedule, everySecs = 3600): Schedule {
  if (current.mode === mode) return current;
  const zone = current.mode === 'every' ? null : current.zone;
  if (mode === 'every') return { mode, secs: everySecs };
  if (mode === 'times') return { mode, days: ['mon', 'tue', 'wed', 'thu', 'fri'], times: ['09:00'], zone };
  return { mode, expr: '0 9 * * 1-5', zone };
}

/** Ticks or unticks a day; the result stays in week order. */
export function toggleDay(days: readonly Weekday[], day: Weekday): Weekday[] {
  const on = days.includes(day) ? days.filter((d) => d !== day) : [...days, day];
  return WEEKDAYS.filter((d) => on.includes(d));
}

/** Adds a time of day an hour after the latest one (wrapping at midnight); nine o'clock for the first. */
export function addTime(times: readonly string[]): string[] {
  const last = [...times].sort().pop();
  const hour = last ? (Number(last.slice(0, 2)) + 1) % 24 : 9;
  return [...times, `${String(hour).padStart(2, '0')}:00`];
}

export function setTime(times: readonly string[], index: number, value: string): string[] {
  return times.map((t, i) => (i === index ? value : t));
}

export function removeTime(times: readonly string[], index: number): string[] {
  return times.filter((_, i) => i !== index);
}

/** The zone text as stored: trimmed, and `null` when empty (the computer's own zone). */
export function zoneValue(text: string): string | null {
  return text.trim() === '' ? null : text.trim();
}

/** A new pause window: every day, 22:00 to 06:00 (a night). */
export function newWindow(): PauseWindow {
  return { days: [], start: '22:00', end: '06:00' };
}

/**
 * A run time as text in the zone the engine reported (`Intl`, the active locale). A zone the runtime does not know (an
 * offset label such as `UTC+02:00`) falls back to the viewer's own zone.
 */
export function formatRun(ms: number, zone: string, locale?: string): string {
  const options: Intl.DateTimeFormatOptions = { weekday: 'short', year: 'numeric', month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' };
  try {
    return new Intl.DateTimeFormat(locale, { ...options, timeZone: zone }).format(ms);
  } catch {
    return new Intl.DateTimeFormat(locale, options).format(ms);
  }
}
