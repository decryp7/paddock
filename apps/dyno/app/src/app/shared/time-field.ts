import { ChangeDetectionStrategy, Component, computed, inject, input, output } from '@angular/core';
import { DateAdapter, MAT_DATE_FORMATS } from '@angular/material/core';
import { MatTimepicker, MatTimepickerInput, MatTimepickerToggle } from '@angular/material/timepicker';
import { PdField, PdFieldControl } from '@decryp7/paddock-ui/forms';
import { dateToTime, PdKeepTimeOnEscape, TIME_PROVIDERS, TimeProblems, timeOptions, timeToDate } from '@decryp7/paddock-ui/schedule';

/**
 * A time of day (24 hours) in a Material field with a list of times: the shared `mat-timepicker` set-up of the schedule editors
 * (`TIME_PROVIDERS`, the Escape guard, the half-hour list) behind one small component. The value is the wall-clock `HH:MM` the
 * engine uses; text the picker cannot read keeps the last good time and shows a problem once the field was left.
 */
@Component({
  selector: 'dy-time-field',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [PdField, PdFieldControl, MatTimepicker, MatTimepickerInput, MatTimepickerToggle, PdKeepTimeOnEscape],
  providers: [TIME_PROVIDERS],
  template: `
    <pd-field [label]="shown() || label()" size="short" [suffix]="toggle" [error]="problems.shown('time') ? 'Enter a time such as 22:00.' : null">
      <input
        pdFieldControl
        pdKeepTime
        [attr.aria-label]="label()"
        [matTimepicker]="picker"
        [value]="date()"
        [disabled]="disabled()"
        autocomplete="off"
        (valueChange)="set($event)"
        (keydown.escape)="closePicker($event, picker)"
        (blur)="problems.leave('time')"
      />
      <mat-timepicker #picker panelClass="pd-timepicker-panel" [options]="list" />
      <ng-template #toggle><mat-timepicker-toggle [for]="picker" [disabled]="disabled()" /></ng-template>
    </pd-field>
  `,
  styles: `
    :host { display: block; min-width: 0; }
    /* The list is wider than the short field it opens from, so a time such as 10:00 stays on one line. */
    ::ng-deep .pd-timepicker-panel { min-width: 8.5rem; }
  `,
})
export class TimeField {
  /** The visible label ("From", "Start of quiet hours"). */
  readonly label = input.required<string>();
  /** A shorter visible label for the narrow field ('From'); the accessible name stays `label`. */
  readonly shown = input('');
  /** The time as `HH:MM`. */
  readonly value = input.required<string>();
  readonly disabled = input(false);
  /** A new valid time as `HH:MM`. */
  readonly timeChange = output<string>();

  private readonly adapter = inject<DateAdapter<Date>>(DateAdapter);
  private readonly formats = inject(MAT_DATE_FORMATS);
  protected readonly list = timeOptions(this.adapter, this.formats);
  protected readonly date = computed(() => timeToDate(this.value()));
  protected readonly problems = new TimeProblems<string>(() => this.value());

  protected set(date: Date | null): void {
    const time = dateToTime(date);
    this.problems.report('time', time !== null);
    if (time && time !== this.value()) this.timeChange.emit(time);
  }

  /** Escape closes an open list and goes no further, so a dialog or window around the field stays open. */
  protected closePicker(e: Event, picker: MatTimepicker<Date>): void {
    if (!picker.isOpen()) return;
    e.stopPropagation();
    picker.close();
  }
}
