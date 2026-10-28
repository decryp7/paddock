import { ChangeDetectionStrategy, Component, computed, inject, input, model } from '@angular/core';
import { MatButton, MatIconButton } from '@angular/material/button';
import { DateAdapter, MAT_DATE_FORMATS } from '@angular/material/core';
import { MatTimepicker, MatTimepickerInput, MatTimepickerToggle } from '@angular/material/timepicker';
import { MatTooltip } from '@angular/material/tooltip';
import { PdField, PdFieldControl } from '@decryp7/paddock-ui/forms';
import { PdGlyph, PdStatusGlyph } from '@decryp7/paddock-ui/icons';
import { PdDayPicker } from './day-picker';
import { MAX_PAUSE_WINDOWS, newWindow, type PauseWindow, type PauseWindows, type Weekday, zoneValue } from './schedule-model';
import { dateToTime, TIME_PROVIDERS, TimeProblems, timeOptions, timeToDate } from './time-value';

/**
 * The shared pause-window editor (plan X33), for pause windows and quiet hours: a list of daily windows (the days they start
 * on, empty is every day; a start after the end spans midnight) in one time zone. Like the schedule editor it holds no
 * rules: the engine validates (`PauseWindows::check`) and the host passes its `problems` back.
 *
 * ```html
 * <pd-pause-editor [(value)]="windows" [problems]="problems()" />
 * ```
 */
@Component({
  selector: 'pd-pause-editor',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [PdStatusGlyph, PdDayPicker, PdGlyph, PdField, PdFieldControl, MatButton, MatIconButton, MatTimepicker, MatTimepickerInput, MatTimepickerToggle, MatTooltip],
  providers: [TIME_PROVIDERS],
  template: `
    @if (value().windows.length === 0) {
      <p class="pd-empty">{{ emptyText }}</p>
    }
    <ul class="pd-windows">
      @for (w of value().windows; track $index) {
        <li class="pd-window" [attr.data-testid]="'pause-window-' + $index">
          <pd-day-picker [value]="w.days" [label]="daysLabel($index + 1)" [disabled]="disabled()" (valueChange)="patch($index, { days: $event })" />
          <pd-field size="short" [label]="fromText" [suffix]="startToggle" [error]="timeProblems.shown($index + 'start') ? badTimeText : null">
            <input pdFieldControl [matTimepicker]="startPicker" [value]="dates()[$index]?.start ?? null" [disabled]="disabled()" autocomplete="off" (valueChange)="setTime($index, 'start', $event)" (keydown.escape)="closePicker($event, startPicker)" (blur)="timeProblems.leave($index + 'start')" />
            <mat-timepicker #startPicker panelClass="pd-timepicker-panel" [options]="timeList" />
            <ng-template #startToggle><mat-timepicker-toggle [for]="startPicker" [disabled]="disabled()" /></ng-template>
          </pd-field>
          <pd-field size="short" [label]="toText" [suffix]="endToggle" [error]="timeProblems.shown($index + 'end') ? badTimeText : null">
            <input pdFieldControl [matTimepicker]="endPicker" [value]="dates()[$index]?.end ?? null" [disabled]="disabled()" autocomplete="off" (valueChange)="setTime($index, 'end', $event)" (keydown.escape)="closePicker($event, endPicker)" (blur)="timeProblems.leave($index + 'end')" />
            <mat-timepicker #endPicker panelClass="pd-timepicker-panel" [options]="timeList" />
            <ng-template #endToggle><mat-timepicker-toggle [for]="endPicker" [disabled]="disabled()" /></ng-template>
          </pd-field>
          @if (w.start > w.end) {
            <span class="pd-note">{{ overnightText }}</span>
          }
          <button type="button" matIconButton [attr.aria-label]="removeLabel($index + 1)" [matTooltip]="removeText" [disabled]="disabled()" (click)="remove($index)">
            <pd-glyph name="close" [size]="14" />
          </button>
        </li>
      }
    </ul>
    <button type="button" matButton="outlined" class="pd-add" [disabled]="disabled() || value().windows.length >= max" (click)="add()"><pd-glyph name="plus" [size]="14" />{{ addText }}</button>
    <pd-field class="pd-zone" data-testid="pause-zone" [label]="zoneLabel">
      <input pdFieldControl type="text" spellcheck="false" autocomplete="off" [placeholder]="zonePlaceholder" [value]="value().zone ?? ''" [disabled]="disabled()" (change)="setZone($event)" />
    </pd-field>
    @for (p of problems(); track $index) {
      <p class="pd-problem" role="alert"><pd-status-glyph status="error" [size]="14" />{{ p }}</p>
    }
  `,
  styles: `
    /* The list is wider than the short field it opens from, so a time such as 10:00 stays on one line. */
    ::ng-deep .pd-timepicker-panel { min-width: 8.5rem; }
    :host { display: grid; gap: var(--pd-space-3); justify-items: start; min-width: 0; }
    :host > .pd-zone { justify-self: stretch; }
    .pd-windows { display: grid; gap: var(--pd-space-2); width: 100%; margin: 0; padding: 0; list-style: none; }
    .pd-window { display: flex; flex-wrap: wrap; align-items: flex-start; gap: var(--pd-space-2); padding: var(--pd-space-2) var(--pd-space-3); border: 1px solid var(--pd-hairline); border-radius: var(--pd-radius-control); background: var(--pd-surface-2); }
    .pd-note, .pd-empty { margin: 0; font-size: var(--pd-text-label); color: var(--pd-text-muted); }
    .pd-problem { display: flex; align-items: flex-start; gap: var(--pd-space-1); margin: 0; font-size: var(--pd-text-label); color: var(--pd-danger); }
  `,
})
export class PdPauseEditor {
  /** The windows being edited. */
  readonly value = model.required<PauseWindows>();
  /** The engine's problems with `value`. */
  readonly problems = input<readonly string[]>([]);
  readonly disabled = input(false);

  private readonly adapter = inject<DateAdapter<Date>>(DateAdapter);
  private readonly formats = inject(MAT_DATE_FORMATS);
  /** The times the picker offers (every 30 minutes). */
  protected readonly timeList = timeOptions(this.adapter, this.formats);
  /** The windows' times as `Date`s, one object per change of `value` (a binding that made a new `Date` on every check would never settle). */
  protected readonly dates = computed(() => this.value().windows.map((w) => ({ start: timeToDate(w.start), end: timeToDate(w.end) })));
  /** The times (`0start`, `1end`...) the picker could not read; the last good time stays in the windows. */
  protected readonly timeProblems = new TimeProblems<string>(() => this.value());
  protected readonly badTimeText = $localize`:Field error|@@pd.schedule.badTime:Enter a time such as 09:00.`;
  protected readonly max = MAX_PAUSE_WINDOWS;
  protected readonly emptyText = $localize`:Empty state|@@pd.schedule.pause.empty:No windows: nothing is paused.`;
  protected readonly fromText = $localize`:Field label|@@pd.schedule.pause.from:From`;
  protected readonly toText = $localize`:Field label|@@pd.schedule.pause.to:Until`;
  protected readonly overnightText = $localize`:Note|@@pd.schedule.pause.overnight:Ends the next day.`;
  protected readonly removeText = $localize`:Button tooltip|@@pd.schedule.pause.remove:Remove this window`;
  protected readonly addText = $localize`:Button|@@pd.schedule.pause.add:Add a window`;
  protected readonly zoneLabel = $localize`:Field label|@@pd.schedule.pause.zone:Time zone`;
  protected readonly zonePlaceholder = $localize`:Field placeholder|@@pd.schedule.pause.zonePlaceholder:This computer's time zone (or an IANA name such as Europe/Berlin)`;

  protected daysLabel(n: number): string {
    return $localize`:Group name|@@pd.schedule.pause.daysN:Days of window ${n}:n: (none selected means every day)`;
  }

  protected removeLabel(n: number): string {
    return $localize`:Button name|@@pd.schedule.pause.removeN:Remove window ${n}:n:`;
  }

  protected patch(index: number, change: { days?: Weekday[]; start?: string; end?: string }): void {
    const v = this.value();
    this.value.set({ ...v, windows: v.windows.map((w, i): PauseWindow => (i === index ? { ...w, ...change } : w)) });
  }

  /** The picker's `Date` (null while the text is empty or incomplete: the last good time stays) as the engine's `HH:MM`. */
  protected setTime(index: number, field: 'start' | 'end', date: Date | null): void {
    const time = dateToTime(date);
    this.timeProblems.report(`${index}${field}`, time !== null);
    if (time) this.patch(index, { [field]: time });
  }

  /** Escape closes an open time list and goes no further, so a dialog around the editor stays open. */
  protected closePicker(e: Event, picker: MatTimepicker<Date>): void {
    if (!picker.isOpen()) return;
    e.stopPropagation();
    picker.close();
  }

  protected add(): void {
    const v = this.value();
    if (v.windows.length < MAX_PAUSE_WINDOWS) this.value.set({ ...v, windows: [...v.windows, newWindow()] });
  }

  protected remove(index: number): void {
    const v = this.value();
    this.value.set({ ...v, windows: v.windows.filter((_, i) => i !== index) });
  }

  protected setZone(e: Event): void {
    this.value.set({ ...this.value(), zone: zoneValue((e.target as HTMLInputElement).value) });
  }
}
