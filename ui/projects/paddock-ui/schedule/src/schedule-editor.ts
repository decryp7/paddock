import { ChangeDetectionStrategy, Component, computed, input, model } from '@angular/core';
import { PdGlyph } from '@decryp7/paddock-ui/icons';
import { PdSelect, type PdSelectOption } from '@decryp7/paddock-ui/select';
import { PdDayPicker } from './day-picker';
import {
  addTime,
  EVERY_PRESETS,
  formatRun,
  INTERVAL_UNITS,
  intervalSecs,
  MAX_TIMES,
  removeTime,
  type IntervalUnit,
  type Schedule,
  type ScheduleMode,
  type SchedulePreview,
  type Weekday,
  scheduleForMode,
  setTime,
  splitInterval,
  zoneValue,
} from './schedule-model';

let nextId = 0;

/** "15 min", "6 h", "Daily": the preset buttons' text. */
export function presetLabel(secs: number): string {
  if (secs === 86_400) return $localize`:Interval preset|@@pd.schedule.preset.daily:Daily`;
  if (secs % 3600 === 0) return $localize`:Interval preset|@@pd.schedule.preset.hours:${secs / 3600}:hours: h`;
  return $localize`:Interval preset|@@pd.schedule.preset.minutes:${secs / 60}:minutes: min`;
}

/**
 * The shared schedule editor (plan X33): pick "Every", "At times" or "Cron", edit it, and see the engine's answer under it
 * (the schedule in words, the next runs in the schedule's zone, warnings and problems). The editor holds no schedule rules:
 * the host asks the engine (`paddock_jobs::schedule::preview` / `check`) on every change and feeds the answer back through
 * `preview` and `problems`, so the text a user reads is the one the engine will act on.
 *
 * ```html
 * <pd-schedule-editor [(value)]="schedule" [preview]="preview()" [problems]="problems()" />
 * ```
 */
@Component({
  selector: 'pd-schedule-editor',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [PdDayPicker, PdGlyph, PdSelect],
  template: `
    <div class="pd-modes" role="radiogroup" [attr.aria-label]="modeLabel">
      @for (m of modes; track m.id) {
        <button type="button" role="radio" class="pd-mode" [class.pd-on]="value().mode === m.id" [attr.aria-checked]="value().mode === m.id" [disabled]="disabled()" (click)="setMode(m.id)">{{ m.label }}</button>
      }
    </div>

    @switch (value().mode) {
      @case ('every') {
        <div class="pd-row" data-testid="schedule-every">
          <div class="pd-presets" role="group" [attr.aria-label]="presetsLabel">
            @for (p of presets; track p) {
              <button type="button" class="pd-preset" [class.pd-on]="everySecs() === p" [attr.aria-pressed]="everySecs() === p" [disabled]="disabled()" (click)="setEvery(p)">{{ label(p) }}</button>
            }
          </div>
          <label class="pd-inline">
            <span>{{ everyLabel }}</span>
            <input class="pd-input pd-input--sm pd-amount" type="number" min="1" step="1" [value]="split().amount" [disabled]="disabled()" (change)="setAmount($event)" />
          </label>
          <pd-select [options]="unitOptions" [value]="split().unit" [ariaLabel]="unitLabel" [disabled]="disabled()" (valueChange)="setUnit($event)" />
        </div>
      }
      @case ('times') {
        <div class="pd-stack" data-testid="schedule-times">
          <pd-day-picker [value]="days()" [label]="daysLabel" [disabled]="disabled()" (valueChange)="setDays($event)" />
          <ul class="pd-times">
            @for (t of times(); track $index) {
              <li>
                <input class="pd-input pd-input--sm" type="time" [value]="t" [attr.aria-label]="timeLabel($index + 1)" [disabled]="disabled()" (change)="setTimeAt($index, $event)" />
                <button type="button" class="pd-btn pd-btn--sm pd-btn--icon pd-btn--ghost" [attr.aria-label]="removeLabel($index + 1)" [attr.title]="removeText" [disabled]="disabled() || times().length < 2" (click)="removeTimeAt($index)">
                  <pd-glyph name="close" [size]="14" />
                </button>
              </li>
            }
          </ul>
          <button type="button" class="pd-btn pd-btn--sm pd-add" [disabled]="disabled() || times().length >= maxTimes" (click)="addTimeOfDay()"><pd-glyph name="plus" [size]="14" />{{ addText }}</button>
        </div>
      }
      @case ('cron') {
        <label class="pd-field" data-testid="schedule-cron">
          <span class="pd-field__label">{{ cronLabel }}</span>
          <input class="pd-input pd-mono" type="text" spellcheck="false" autocomplete="off" [value]="cron()" [disabled]="disabled()" (change)="setCron($event)" />
          <span class="pd-field__hint">{{ cronHint }}</span>
        </label>
      }
    }

    @if (value().mode !== 'every') {
      <label class="pd-field">
        <span class="pd-field__label">{{ zoneLabel }}</span>
        <input class="pd-input" type="text" spellcheck="false" autocomplete="off" [placeholder]="zonePlaceholder" [value]="zone() ?? ''" [disabled]="disabled()" (change)="setZone($event)" />
      </label>
    }

    <section class="pd-preview" aria-live="polite" [attr.aria-label]="previewLabel" data-testid="schedule-preview">
      @for (p of problems(); track $index) {
        <p class="pd-problem" role="alert"><pd-glyph name="alert" [size]="14" />{{ p }}</p>
      }
      @if (problems().length === 0 && preview(); as pv) {
        <p class="pd-description">{{ pv.description }}</p>
        @for (w of pv.warnings; track $index) {
          <p class="pd-warning"><pd-glyph name="info" [size]="14" />{{ w }}</p>
        }
        <ol class="pd-runs" [attr.aria-label]="nextRunsLabel">
          @for (r of pv.nextRuns; track $index) {
            <li>{{ run(r, pv.zone) }}</li>
          }
        </ol>
        <p class="pd-zone">{{ zoneIn(pv.zone) }}</p>
      }
    </section>
  `,
  styles: `
    :host { display: grid; gap: var(--pd-space-3); min-width: 0; }
    .pd-modes { display: inline-flex; gap: 2px; justify-self: start; max-width: 100%; padding: 2px; border: 1px solid var(--pd-hairline); border-radius: var(--pd-radius-control); background: var(--pd-surface-2); }
    .pd-mode, .pd-preset { min-height: var(--pd-control-height); padding: 0 var(--pd-space-3); border: 1px solid transparent; border-radius: calc(var(--pd-radius-control) - 2px); background: transparent; color: var(--pd-text-muted); font: inherit; font-size: var(--pd-text-label); font-weight: var(--pd-weight-medium); }
    .pd-preset { padding: 0 var(--pd-space-2); border-color: var(--pd-border-control); background: var(--pd-surface-1); }
    .pd-mode:hover:not(:disabled, .pd-on), .pd-preset:hover:not(:disabled, .pd-on) { background: var(--pd-hover); color: var(--pd-text); }
    .pd-mode.pd-on, .pd-preset.pd-on { border-color: var(--pd-accent); background: var(--pd-accent-tint); color: var(--pd-text); font-weight: var(--pd-weight-semibold); }
    .pd-mode:focus-visible, .pd-preset:focus-visible { outline: 2px solid var(--pd-focus-ring); outline-offset: 2px; }
    .pd-mode:disabled, .pd-preset:disabled { opacity: 0.5; }
    .pd-row, .pd-stack { display: flex; flex-wrap: wrap; align-items: center; gap: var(--pd-space-2); min-width: 0; }
    .pd-stack { display: grid; justify-items: start; }
    .pd-presets { display: inline-flex; flex-wrap: wrap; gap: var(--pd-space-1); }
    .pd-inline { display: inline-flex; align-items: center; gap: var(--pd-space-2); font-size: var(--pd-text-label); color: var(--pd-text-muted); }
    .pd-amount { width: 6ch; }
    .pd-times { display: grid; gap: var(--pd-space-1); margin: 0; padding: 0; list-style: none; }
    .pd-times li { display: flex; align-items: center; gap: var(--pd-space-1); }
    .pd-add { gap: var(--pd-space-1); }
    .pd-preview { display: grid; gap: var(--pd-space-1); min-width: 0; padding: var(--pd-space-3); border: 1px solid var(--pd-hairline); border-radius: var(--pd-radius-control); background: var(--pd-surface-2); }
    .pd-preview p, .pd-runs { margin: 0; }
    .pd-description { font-weight: var(--pd-weight-semibold); }
    .pd-problem, .pd-warning { display: flex; align-items: flex-start; gap: var(--pd-space-1); font-size: var(--pd-text-label); }
    .pd-problem { color: var(--pd-danger); }
    .pd-warning { color: var(--pd-text-muted); }
    .pd-runs { padding-inline-start: var(--pd-space-5); font-variant-numeric: tabular-nums; }
    .pd-zone { font-size: var(--pd-text-caption); color: var(--pd-text-muted); }
  `,
})
export class PdScheduleEditor {
  /** The schedule being edited. */
  readonly value = model.required<Schedule>();
  /** The engine's preview of `value` (null while it is being asked). */
  readonly preview = input<SchedulePreview | null>(null);
  /** The engine's problems with `value`; the preview is hidden while there are any. */
  readonly problems = input<readonly string[]>([]);
  readonly disabled = input(false);
  /** A locale for the run times; the runtime's default when empty. */
  readonly locale = input<string | undefined>(undefined);

  protected readonly id = `pd-schedule-${++nextId}`;
  protected readonly maxTimes = MAX_TIMES;
  protected readonly presets = EVERY_PRESETS;
  protected readonly label = presetLabel;
  protected readonly modes: readonly { id: ScheduleMode; label: string }[] = [
    { id: 'every', label: $localize`:Schedule mode|@@pd.schedule.mode.every:Every…` },
    { id: 'times', label: $localize`:Schedule mode|@@pd.schedule.mode.times:At times` },
    { id: 'cron', label: $localize`:Schedule mode|@@pd.schedule.mode.cron:Cron` },
  ];
  protected readonly unitOptions: readonly PdSelectOption[] = INTERVAL_UNITS.map((u) => ({
    value: u.id,
    label: u.id === 'minutes' ? $localize`:Interval unit|@@pd.schedule.unit.minutes:Minutes` : u.id === 'hours' ? $localize`:Interval unit|@@pd.schedule.unit.hours:Hours` : $localize`:Interval unit|@@pd.schedule.unit.days:Days`,
  }));
  protected readonly modeLabel = $localize`:Group name|@@pd.schedule.modeLabel:Schedule type`;
  protected readonly presetsLabel = $localize`:Group name|@@pd.schedule.presetsLabel:Common intervals`;
  protected readonly everyLabel = $localize`:Field label|@@pd.schedule.every:Every`;
  protected readonly unitLabel = $localize`:Field name|@@pd.schedule.unitLabel:Interval unit`;
  protected readonly daysLabel = $localize`:Group name|@@pd.schedule.daysLabel:Days of the week`;
  protected readonly removeText = $localize`:Button tooltip|@@pd.schedule.removeTime:Remove this time`;
  protected readonly addText = $localize`:Button|@@pd.schedule.addTime:Add a time`;
  protected readonly cronLabel = $localize`:Field label|@@pd.schedule.cron:Cron expression`;
  protected readonly cronHint = $localize`:Field hint|@@pd.schedule.cronHint:Five fields: minute hour day-of-month month day-of-week, for example 0 9 * * 1-5. A leading seconds field is allowed.`;
  protected readonly zoneLabel = $localize`:Field label|@@pd.schedule.zone:Time zone`;
  protected readonly zonePlaceholder = $localize`:Field placeholder|@@pd.schedule.zonePlaceholder:This computer's time zone (or an IANA name such as Europe/Berlin)`;
  protected readonly previewLabel = $localize`:Region name|@@pd.schedule.previewLabel:Schedule preview`;
  protected readonly nextRunsLabel = $localize`:List name|@@pd.schedule.nextRuns:Next runs`;

  protected readonly everySecs = computed(() => {
    const v = this.value();
    return v.mode === 'every' ? v.secs : 0;
  });
  protected readonly split = computed(() => splitInterval(this.everySecs() || 3600));
  protected readonly days = computed(() => {
    const v = this.value();
    return v.mode === 'times' ? v.days : [];
  });
  protected readonly times = computed(() => {
    const v = this.value();
    return v.mode === 'times' ? v.times : [];
  });
  protected readonly cron = computed(() => {
    const v = this.value();
    return v.mode === 'cron' ? v.expr : '';
  });
  protected readonly zone = computed(() => {
    const v = this.value();
    return v.mode === 'every' ? null : v.zone;
  });

  protected timeLabel(n: number): string {
    return $localize`:Field name|@@pd.schedule.timeN:Time of day ${n}:n:`;
  }

  protected removeLabel(n: number): string {
    return $localize`:Button name|@@pd.schedule.removeTimeN:Remove time of day ${n}:n:`;
  }

  protected zoneIn(zone: string): string {
    return $localize`:Preview note|@@pd.schedule.zoneIn:Times are in ${zone}:zone:.`;
  }

  protected run(ms: number, zone: string): string {
    return formatRun(ms, zone, this.locale());
  }

  protected setMode(mode: ScheduleMode): void {
    this.value.set(scheduleForMode(mode, this.value()));
  }

  protected setEvery(secs: number): void {
    this.value.set({ mode: 'every', secs });
  }

  protected setAmount(e: Event): void {
    const secs = intervalSecs(Number((e.target as HTMLInputElement).value), this.split().unit);
    if (secs !== null) this.setEvery(secs);
    else (e.target as HTMLInputElement).value = String(this.split().amount);
  }

  protected setUnit(unit: string): void {
    const secs = intervalSecs(this.split().amount, unit as IntervalUnit);
    if (secs !== null) this.setEvery(secs);
  }

  protected setDays(days: readonly Weekday[]): void {
    const v = this.value();
    if (v.mode === 'times') this.value.set({ ...v, days });
  }

  protected addTimeOfDay(): void {
    const v = this.value();
    if (v.mode === 'times') this.value.set({ ...v, times: addTime(v.times) });
  }

  protected setTimeAt(index: number, e: Event): void {
    const v = this.value();
    const text = (e.target as HTMLInputElement).value;
    if (v.mode === 'times' && text) this.value.set({ ...v, times: setTime(v.times, index, text) });
  }

  protected removeTimeAt(index: number): void {
    const v = this.value();
    if (v.mode === 'times' && v.times.length > 1) this.value.set({ ...v, times: removeTime(v.times, index) });
  }

  protected setCron(e: Event): void {
    const v = this.value();
    if (v.mode === 'cron') this.value.set({ ...v, expr: (e.target as HTMLInputElement).value });
  }

  protected setZone(e: Event): void {
    const v = this.value();
    if (v.mode !== 'every') this.value.set({ ...v, zone: zoneValue((e.target as HTMLInputElement).value) });
  }
}
