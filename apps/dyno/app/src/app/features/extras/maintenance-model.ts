import type { MaintenanceSettings, MaintenanceStatus, MaintenanceWindow, Weekday } from '../../core/api.types';
import { formatClock, formatDayTime } from '../../shared/format';

/** The most windows the engine accepts. */
export const MAX_WINDOWS = 14;

export const WEEKDAYS: readonly { day: Weekday; short: string; long: string }[] = [
  { day: 'mon', short: 'Mon', long: 'Monday' },
  { day: 'tue', short: 'Tue', long: 'Tuesday' },
  { day: 'wed', short: 'Wed', long: 'Wednesday' },
  { day: 'thu', short: 'Thu', long: 'Thursday' },
  { day: 'fri', short: 'Fri', long: 'Friday' },
  { day: 'sat', short: 'Sat', long: 'Saturday' },
  { day: 'sun', short: 'Sun', long: 'Sunday' },
];

/** 1320 as "22:00". */
export function minuteToTime(minute: number): string {
  const m = Math.max(0, Math.min(1439, Math.round(minute)));
  return `${String(Math.floor(m / 60)).padStart(2, '0')}:${String(m % 60).padStart(2, '0')}`;
}

/** "22:00" as 1320; null when it is not a time of day. */
export function timeToMinute(text: string): number | null {
  const m = /^(\d{1,2}):(\d{2})$/.exec(text.trim());
  if (!m) return null;
  const h = Number(m[1]);
  const min = Number(m[2]);
  return h < 24 && min < 60 ? h * 60 + min : null;
}

/** A new window the person can edit: weeknights from 22:00 to 02:00. */
export function newWindow(): MaintenanceWindow {
  return { days: ['mon', 'tue', 'wed', 'thu', 'fri'], startMinute: 22 * 60, endMinute: 2 * 60 };
}

/** The settings with `w` replacing the window at `index`. */
export function withWindow(s: MaintenanceSettings, index: number, w: MaintenanceWindow): MaintenanceSettings {
  return { ...s, windows: s.windows.map((x, i) => (i === index ? w : x)) };
}

export function addWindow(s: MaintenanceSettings): MaintenanceSettings {
  return s.windows.length >= MAX_WINDOWS ? s : { ...s, windows: [...s.windows, newWindow()] };
}

export function removeWindow(s: MaintenanceSettings, index: number): MaintenanceSettings {
  return { ...s, windows: s.windows.filter((_, i) => i !== index) };
}

/** Adds or removes a weekday (kept in week order). */
export function toggleDay(w: MaintenanceWindow, day: Weekday): MaintenanceWindow {
  const on = w.days.includes(day);
  const days = WEEKDAYS.map((d) => d.day).filter((d) => (d === day ? !on : w.days.includes(d)));
  return { ...w, days };
}

/** Early, convenient validation with the engine's sentences; the engine checks again and its message is shown when it disagrees. */
export function validateMaintenance(s: MaintenanceSettings): string | null {
  if (s.windows.length > MAX_WINDOWS) return `At most ${MAX_WINDOWS} maintenance windows.`;
  if (s.enabled && s.windows.length === 0) return 'Add a window, or turn maintenance windows off.';
  for (const w of s.windows) {
    if (w.days.length === 0) return 'Pick at least one day for each window.';
    if (!(w.startMinute >= 0 && w.startMinute < 1440 && w.endMinute >= 0 && w.endMinute < 1440)) return "A window's times are between 00:00 and 23:59.";
    if (w.startMinute === w.endMinute) return 'A window must end at a different time than it starts.';
  }
  return null;
}

/** "Mon to Fri", "Sat, Sun" or "Every day": consecutive days collapse into a range. */
export function daysText(days: readonly Weekday[]): string {
  const idx = WEEKDAYS.map((d, i) => (days.includes(d.day) ? i : -1)).filter((i) => i >= 0);
  if (idx.length === 7) return 'Every day';
  const parts: string[] = [];
  for (let i = 0; i < idx.length; ) {
    let j = i;
    while (j + 1 < idx.length && idx[j + 1] === idx[j]! + 1) j++;
    const a = WEEKDAYS[idx[i]!]!.short;
    const b = WEEKDAYS[idx[j]!]!.short;
    parts.push(j - i >= 2 ? `${a} to ${b}` : j - i === 1 ? `${a}, ${b}` : a);
    i = j + 1;
  }
  return parts.join(', ');
}

/** "Mon to Fri, 22:00 to 02:00 the next day". */
export function windowText(w: MaintenanceWindow): string {
  const over = w.endMinute <= w.startMinute;
  return `${daysText(w.days)}, ${minuteToTime(w.startMinute)} to ${minuteToTime(w.endMinute)}${over ? ' the next day' : ''}`;
}

/** The status line: "Open now, until 02:00", "Next window opens Sat 23:00" or that nothing is limited. */
export function statusText(s: MaintenanceStatus): string {
  if (!s.enabled) return 'Not limited: heavier background work can run at any time.';
  if (s.openNow) return s.closesAt === null ? 'Open now' : `Open now, until ${formatClock(s.closesAt)}`;
  return s.nextOpensAt === null ? 'Closed; no window opens in the coming week' : `Closed now. Next window opens ${formatDayTime(s.nextOpensAt)}`;
}
