import { ChangeDetectionStrategy, Component, computed, input, output, signal } from '@angular/core';
import { MatIcon } from '@angular/material/icon';
import type { Schedule, SchedulePreview, ScheduleMode, Weekday } from '../../core/api.types';
import { formatDateTime } from '../../shared/format';
import {
  addTime,
  EVERY_PRESETS,
  INTERVAL_UNITS,
  intervalSecs,
  removeTime,
  SCHEDULE_MODES,
  schedulePerMode,
  setTime,
  splitInterval,
  toggleDay,
  WEEKDAYS,
  zoneValue,
  type IntervalUnit,
} from '../../shared/schedule';
import { AgoPipe } from '../../shared/ui/ago.pipe';
import type { Loadable } from '../../state/loadable';

/**
 * Chooses when a watch is checked: every so often, at times of day, or a cron expression. It only edits the schedule
 * value; the description, the next runs and every complaint come from the engine (`preview`).
 */
@Component({
  selector: 'sp-schedule-editor',
  imports: [MatIcon, AgoPipe],
  changeDetection: ChangeDetectionStrategy.OnPush,
  templateUrl: './schedule-editor.html',
  styleUrl: './schedule-editor.scss',
})
export class ScheduleEditor {
  readonly schedule = input.required<Schedule>();
  /** The engine's answer for the current schedule. */
  readonly preview = input.required<Loadable<SchedulePreview>>();
  readonly scheduleChange = output<Schedule>();

  protected readonly modes = SCHEDULE_MODES;
  protected readonly presets = EVERY_PRESETS;
  protected readonly units = INTERVAL_UNITS;
  protected readonly days = WEEKDAYS;
  protected readonly dateTime = formatDateTime;

  /** The interval last chosen, kept while another mode is shown. */
  private readonly lastEvery = signal(6 * 3600);

  protected readonly every = computed(() => {
    const s = this.schedule();
    return s.mode === 'every' ? s : null;
  });
  protected readonly times = computed(() => {
    const s = this.schedule();
    return s.mode === 'times' ? s : null;
  });
  protected readonly cron = computed(() => {
    const s = this.schedule();
    return s.mode === 'cron' ? s : null;
  });
  protected readonly custom = computed(() => splitInterval(this.every()?.secs ?? this.lastEvery()));
  protected readonly isPreset = computed(() => EVERY_PRESETS.some((p) => p.secs === this.every()?.secs));
  protected readonly nextMs = computed(() => this.preview().value?.nextRunsMs[0] ?? null);
  protected readonly problems = computed(() => {
    const e = this.preview().error;
    return e ? (e.problems.length ? e.problems : [e.message]) : [];
  });

  protected setMode(mode: ScheduleMode): void {
    const current = this.schedule();
    if (current.mode === 'every') this.lastEvery.set(current.secs);
    this.scheduleChange.emit(schedulePerMode(mode, current, this.lastEvery()));
  }

  protected setEvery(secs: number): void {
    this.lastEvery.set(secs);
    this.scheduleChange.emit({ mode: 'every', secs });
  }

  protected setCustom(amountText: string, unit: IntervalUnit, input?: HTMLInputElement): void {
    const secs = intervalSecs(Number(amountText), unit);
    if (secs !== null) this.setEvery(secs);
    // An unusable number goes back to the value in use.
    else if (input) input.value = String(this.custom().amount);
  }

  protected toggle(day: Weekday): void {
    const s = this.times();
    if (s) this.scheduleChange.emit({ ...s, days: toggleDay(s.days, day) });
  }

  protected add(): void {
    const s = this.times();
    if (s) this.scheduleChange.emit({ ...s, times: addTime(s.times) });
  }

  protected change(index: number, value: string): void {
    const s = this.times();
    if (s && value) this.scheduleChange.emit({ ...s, times: setTime(s.times, index, value) });
  }

  protected remove(index: number): void {
    const s = this.times();
    if (s && s.times.length > 1) this.scheduleChange.emit({ ...s, times: removeTime(s.times, index) });
  }

  protected setZone(text: string): void {
    const s = this.schedule();
    if (s.mode !== 'every') this.scheduleChange.emit({ ...s, zone: zoneValue(text) });
  }

  protected setExpr(expr: string): void {
    const s = this.cron();
    if (s) this.scheduleChange.emit({ ...s, expr });
  }

  protected input(e: Event): HTMLInputElement {
    return e.target as HTMLInputElement;
  }

  protected value(e: Event): string {
    return (e.target as HTMLInputElement).value;
  }
}
