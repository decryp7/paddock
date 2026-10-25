import { ChangeDetectionStrategy, Component, input, model } from '@angular/core';
import { PdGlyph, PdStatusGlyph } from '@decryp7/paddock-ui/icons';
import { PdDayPicker } from './day-picker';
import { MAX_PAUSE_WINDOWS, newWindow, type PauseWindow, type PauseWindows, type Weekday, zoneValue } from './schedule-model';

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
  imports: [PdStatusGlyph, PdDayPicker, PdGlyph],
  template: `
    @if (value().windows.length === 0) {
      <p class="pd-empty">{{ emptyText }}</p>
    }
    <ul class="pd-windows">
      @for (w of value().windows; track $index) {
        <li class="pd-window" [attr.data-testid]="'pause-window-' + $index">
          <pd-day-picker [value]="w.days" [label]="daysLabel($index + 1)" [disabled]="disabled()" (valueChange)="patch($index, { days: $event })" />
          <label class="pd-inline">
            <span>{{ fromText }}</span>
            <input class="pd-input pd-input--sm" type="time" [value]="w.start" [disabled]="disabled()" (change)="setTime($index, 'start', $event)" />
          </label>
          <label class="pd-inline">
            <span>{{ toText }}</span>
            <input class="pd-input pd-input--sm" type="time" [value]="w.end" [disabled]="disabled()" (change)="setTime($index, 'end', $event)" />
          </label>
          @if (w.start > w.end) {
            <span class="pd-note">{{ overnightText }}</span>
          }
          <button type="button" class="pd-btn pd-btn--sm pd-btn--icon pd-btn--ghost" [attr.aria-label]="removeLabel($index + 1)" [attr.title]="removeText" [disabled]="disabled()" (click)="remove($index)">
            <pd-glyph name="close" [size]="14" />
          </button>
        </li>
      }
    </ul>
    <button type="button" class="pd-btn pd-btn--sm pd-add" [disabled]="disabled() || value().windows.length >= max" (click)="add()"><pd-glyph name="plus" [size]="14" />{{ addText }}</button>
    <label class="pd-field">
      <span class="pd-field__label">{{ zoneLabel }}</span>
      <input class="pd-input" type="text" spellcheck="false" autocomplete="off" [placeholder]="zonePlaceholder" [value]="value().zone ?? ''" [disabled]="disabled()" (change)="setZone($event)" />
    </label>
    @for (p of problems(); track $index) {
      <p class="pd-problem" role="alert"><pd-status-glyph status="error" [size]="14" />{{ p }}</p>
    }
  `,
  styles: `
    :host { display: grid; gap: var(--pd-space-3); justify-items: start; min-width: 0; }
    :host > .pd-field { justify-self: stretch; }
    .pd-windows { display: grid; gap: var(--pd-space-2); width: 100%; margin: 0; padding: 0; list-style: none; }
    .pd-window { display: flex; flex-wrap: wrap; align-items: center; gap: var(--pd-space-2); padding: var(--pd-space-2) var(--pd-space-3); border: 1px solid var(--pd-hairline); border-radius: var(--pd-radius-control); background: var(--pd-surface-2); }
    .pd-inline { display: inline-flex; align-items: center; gap: var(--pd-space-1); font-size: var(--pd-text-label); color: var(--pd-text-muted); }
    .pd-note, .pd-empty { margin: 0; font-size: var(--pd-text-label); color: var(--pd-text-muted); }
    .pd-add { gap: var(--pd-space-1); }
    .pd-problem { display: flex; align-items: flex-start; gap: var(--pd-space-1); margin: 0; font-size: var(--pd-text-label); color: var(--pd-danger); }
  `,
})
export class PdPauseEditor {
  /** The windows being edited. */
  readonly value = model.required<PauseWindows>();
  /** The engine's problems with `value`. */
  readonly problems = input<readonly string[]>([]);
  readonly disabled = input(false);

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

  protected setTime(index: number, field: 'start' | 'end', e: Event): void {
    const text = (e.target as HTMLInputElement).value;
    if (text) this.patch(index, { [field]: text });
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
