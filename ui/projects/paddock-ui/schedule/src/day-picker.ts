import { ChangeDetectionStrategy, Component, input, output } from '@angular/core';
import { toggleDay, WEEKDAYS, type Weekday } from './schedule-model';

/** The short name of a day, in the active locale (English source text). */
export function dayLabel(day: Weekday): string {
  switch (day) {
    case 'mon':
      return $localize`:Short weekday|@@pd.schedule.day.mon:Mon`;
    case 'tue':
      return $localize`:Short weekday|@@pd.schedule.day.tue:Tue`;
    case 'wed':
      return $localize`:Short weekday|@@pd.schedule.day.wed:Wed`;
    case 'thu':
      return $localize`:Short weekday|@@pd.schedule.day.thu:Thu`;
    case 'fri':
      return $localize`:Short weekday|@@pd.schedule.day.fri:Fri`;
    case 'sat':
      return $localize`:Short weekday|@@pd.schedule.day.sat:Sat`;
    case 'sun':
      return $localize`:Short weekday|@@pd.schedule.day.sun:Sun`;
  }
}

/** Seven day toggles (Monday first) in a labelled group; each is a real button with `aria-pressed`. */
@Component({
  selector: 'pd-day-picker',
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: { role: 'group', '[attr.aria-label]': 'label()' },
  template: `
    @for (d of days; track d) {
      <button type="button" class="pd-day" [class.pd-on]="value().includes(d)" [attr.aria-pressed]="value().includes(d)" [disabled]="disabled()" (click)="valueChange.emit(toggle(d))">{{ name(d) }}</button>
    }
  `,
  styles: `
    :host { display: inline-flex; flex-wrap: wrap; gap: var(--pd-space-1); }
    .pd-day { min-width: var(--pd-hit-min); min-height: var(--pd-control-height); padding: 0 var(--pd-space-2); border: 1px solid var(--pd-border-control); border-radius: var(--pd-radius-control); background: var(--pd-surface-1); color: var(--pd-text-muted); font: inherit; font-size: var(--pd-text-label); }
    .pd-day:hover:not(:disabled, .pd-on) { background: var(--pd-hover); color: var(--pd-text); }
    .pd-day.pd-on { border-color: var(--pd-accent); background: var(--pd-accent-tint); color: var(--pd-text); font-weight: var(--pd-weight-semibold); }
    .pd-day:focus-visible { outline: 2px solid var(--pd-focus-ring); outline-offset: 2px; }
    .pd-day:disabled { opacity: 0.5; }
  `,
})
export class PdDayPicker {
  /** The chosen days. */
  readonly value = input.required<readonly Weekday[]>();
  /** The group's accessible name. */
  readonly label = input.required<string>();
  readonly disabled = input(false);
  /** The new set of days, in week order. */
  readonly valueChange = output<Weekday[]>();
  protected readonly days = WEEKDAYS;
  protected readonly name = dayLabel;

  protected toggle(day: Weekday): Weekday[] {
    return toggleDay(this.value(), day);
  }
}
