import { type DateAdapter, MAT_DATE_FORMATS, MAT_NATIVE_DATE_FORMATS, type MatDateFormats, provideNativeDateAdapter } from '@angular/material/core';
import { linkedSignal, type WritableSignal } from '@angular/core';
import type { MatTimepickerOption } from '@angular/material/timepicker';

/**
 * Time-of-day values for the schedule editors. The engine's wire form is a wall-clock `"HH:MM"` (24 hour, no date, no zone);
 * `mat-timepicker` edits a `Date`. The two are converted here, at the edge, on one fixed reference day, so a time is never
 * moved by the zone's offset or by a daylight-saving change: the timepicker keeps the date part of the value it was given and
 * changes only the hours and minutes, and the reference day (1 January 2000) is not a clock-change day in the zones tested. Reading a value back uses the local hours and minutes (never UTC or the day), so whatever day a `Date`
 * carries, its wall-clock time is what is stored. The one thing the picker cannot do is name a wall-clock time that does not
 * exist on the day the user typed it (02:30 on the day the clocks go forward): such text is parsed by the platform as 03:30.
 */
const HH_MM = /^([01]\d|2[0-3]):([0-5]\d)$/;

/** The reference day's `Date` for an `"HH:MM"` time, or null for anything else (an empty or malformed value). */
export function timeToDate(time: string | null | undefined): Date | null {
  const m = HH_MM.exec(time ?? '');
  return m ? new Date(2000, 0, 1, Number(m[1]), Number(m[2]), 0, 0) : null;
}

/** The `"HH:MM"` of a `Date`'s local wall-clock time, or null for no or an invalid date. */
export function dateToTime(date: Date | null | undefined): string | null {
  if (!date || Number.isNaN(date.getTime())) return null;
  return `${String(date.getHours()).padStart(2, '0')}:${String(date.getMinutes()).padStart(2, '0')}`;
}

/**
 * The picker's list: every `stepMinutes` of the reference day, labelled by the adapter's locale. Passed as the picker's
 * `options`, so the list is complete on the days the clocks change (the picker's own list is built from today).
 */
export function timeOptions(adapter: DateAdapter<Date>, formats: MatDateFormats, stepMinutes = 30): MatTimepickerOption<Date>[] {
  const options: MatTimepickerOption<Date>[] = [];
  for (let minutes = 0; minutes < 24 * 60; minutes += stepMinutes) {
    const value = new Date(2000, 0, 1, Math.floor(minutes / 60), minutes % 60, 0, 0);
    options.push({ value, label: adapter.format(value, formats.display.timeOptionLabel) });
  }
  return options;
}

const TIME_24H: Intl.DateTimeFormatOptions = { hour: '2-digit', minute: '2-digit', hourCycle: 'h23' };

/** The times are shown and read in 24 hours ("17:45"), like the engine's `HH:MM` and the preview under the editor. */
export const TIME_FORMATS: MatDateFormats = {
  ...MAT_NATIVE_DATE_FORMATS,
  parse: { ...MAT_NATIVE_DATE_FORMATS.parse, timeInput: TIME_24H },
  display: { ...MAT_NATIVE_DATE_FORMATS.display, timeInput: TIME_24H, timeOptionLabel: TIME_24H },
};

/** What the timepicker needs: the platform's `Date` adapter, in the app's locale, with 24-hour times. */
export const TIME_PROVIDERS = [provideNativeDateAdapter(TIME_FORMATS), { provide: MAT_DATE_FORMATS, useValue: TIME_FORMATS }];

/**
 * The time fields whose text the picker could not read (empty, or not a time). A field is marked as it is typed in but
 * shows the problem only once it was left (like every field's error); a valid time clears it; and any change of the
 * schedule from outside the field clears all of them, because the fields are then rewritten with their real times.
 */
export class TimeProblems<K> {
  private readonly typed: WritableSignal<ReadonlySet<K>>;
  private readonly left: WritableSignal<ReadonlySet<K>>;

  /** `source` is what the fields are rendered from (the editor's `value`). */
  constructor(source: () => unknown) {
    this.typed = linkedSignal({ source, computation: (): ReadonlySet<K> => new Set<K>() });
    this.left = linkedSignal({ source, computation: (): ReadonlySet<K> => new Set<K>() });
  }

  /** The picker reported the field's text: `ok` when it read a time. */
  report(key: K, ok: boolean): void {
    this.typed.update((s) => with_(s, key, !ok));
    if (ok) this.left.update((s) => with_(s, key, false));
  }

  /** The field was left. */
  leave(key: K): void {
    if (this.typed().has(key)) this.left.update((s) => with_(s, key, true));
  }

  /** Whether the field shows its problem. */
  shown(key: K): boolean {
    return this.left().has(key);
  }
}

function with_<K>(set: ReadonlySet<K>, key: K, present: boolean): ReadonlySet<K> {
  if (set.has(key) === present) return set;
  const next = new Set(set);
  if (present) next.add(key);
  else next.delete(key);
  return next;
}
