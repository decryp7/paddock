import { ChangeDetectionStrategy, Component, input, output } from '@angular/core';
import { MatButtonToggle, MatButtonToggleChange, MatButtonToggleGroup } from '@angular/material/button-toggle';
import { WEEKDAYS, type Weekday } from './schedule-model';

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

/**
 * Seven day toggles (Monday first) in a labelled group: a multiple-selection `mat-button-toggle-group`, so each day is a real
 * button with `aria-pressed`; Tab moves between the days (Material's toggles are separate tab stops) and Space or Enter flips one.
 */
@Component({
  selector: 'pd-day-picker',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [MatButtonToggleGroup, MatButtonToggle],
  template: `
    <mat-button-toggle-group multiple hideMultipleSelectionIndicator [aria-label]="label()" [value]="value()" [disabled]="disabled()" (change)="changed($event)">
      @for (d of days; track d) {
        <mat-button-toggle [value]="d">{{ name(d) }}</mat-button-toggle>
      }
    </mat-button-toggle-group>
  `,
  styles: `
    :host { display: inline-flex; }
    /* Two selected days side by side share one ring: the second leaves out its left edge. */
    :host ::ng-deep .mat-button-toggle-checked + .mat-button-toggle-checked { box-shadow: inset 0 1px 0 0 var(--pd-accent), inset -1px 0 0 0 var(--pd-accent), inset 0 -1px 0 0 var(--pd-accent); }
    :host ::ng-deep .mat-button-toggle-disabled:not(.mat-button-toggle-checked) { opacity: 0.5; }
    /* A disabled chosen option keeps its ring and tint at full strength and dims only the label, so what is chosen stays readable. */
    :host ::ng-deep .mat-button-toggle-disabled.mat-button-toggle-checked .mat-button-toggle-label-content { opacity: 0.6; }
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

  protected changed(e: MatButtonToggleChange): void {
    const chosen = e.value as readonly Weekday[];
    this.valueChange.emit(WEEKDAYS.filter((d) => chosen.includes(d)));
  }
}
