import { ChangeDetectionStrategy, Component, computed, input, model, output } from '@angular/core';
import type { FormValueControl, ValidationError } from '@angular/forms/signals';
import { type SettingField, type SettingValue, displayValue, editUnit, parseList, parseNumber, rangeHint, sameValue } from '@decryp7/paddock-ui/shell/settings';

let nextId = 0;

/**
 * One setting rendered from its schema: a native control per kind (switch, select, text, number with its unit, list),
 * its limits, help, a "Needs restart" badge and Reset to default. A Signal Forms control (`[formField]`): the form owns
 * the value and its errors (the pre-check's or the engine's refusal). `commit` asks to apply: at once for a switch or a
 * choice, on Enter or leaving the control for typed values.
 */
@Component({
  selector: 'pd-setting-field',
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: { class: 'pd-setting', '[class.invalid]': '!!message()', '[class.edited]': 'dirty()', '[attr.data-key]': 'setting().key' },
  template: `
    @let f = setting();
    @let k = f.kind;
    <div class="head">
      @if (k.type === 'bool') {
        <input class="switch" type="checkbox" role="switch" [id]="id" [checked]="value() === true" [disabled]="disabled()" [attr.aria-describedby]="describedBy()" [attr.aria-invalid]="!!message()" (change)="choose($any($event.target).checked)" (blur)="touch.emit()" />
      }
      <label class="label" [for]="id">{{ f.label }}</label>
      @if (f.restartRequired) {
        <span class="badge" title="Takes effect after a restart">Needs restart</span>
      }
      @if (f.advanced) {
        <span class="badge">Advanced</span>
      }
      <span class="spacer"></span>
      @if (!disabled() && !isDefault()) {
        <button class="reset" type="button" [attr.aria-label]="'Reset to default: ' + f.label" [title]="'Default: ' + defaultText()" (click)="resetToDefault.emit()">Reset to default</button>
      }
    </div>
    @if (k.type !== 'bool') {
      <div class="control">
        @switch (k.type) {
          @case ('enum') {
            <select class="input" [id]="id" [disabled]="disabled()" [attr.aria-describedby]="describedBy()" [attr.aria-invalid]="!!message()" (change)="choose($any($event.target).value)" (blur)="touch.emit()">
              @for (o of k.options; track o.value) {
                <option [value]="o.value" [selected]="o.value === value()">{{ o.label }}</option>
              }
            </select>
          }
          @case ('stringList') {
            <textarea class="input wide" rows="3" [id]="id" [value]="listText()" [disabled]="disabled()" [attr.aria-describedby]="describedBy()" [attr.aria-invalid]="!!message()" (input)="value.set(list($any($event.target).value))" (blur)="leave()"></textarea>
          }
          @case ('text') {
            <input class="input wide" type="text" [id]="id" [value]="text()" [disabled]="disabled()" [attr.aria-describedby]="describedBy()" [attr.aria-invalid]="!!message()" (input)="value.set($any($event.target).value)" (keydown.enter)="commit.emit()" (blur)="leave()" />
          }
          @case ('path') {
            <input class="input wide mono" type="text" [id]="id" [value]="text()" [disabled]="disabled()" [placeholder]="k.dir ? 'Folder (empty: the default)' : 'File (empty: the default)'" [attr.aria-describedby]="describedBy()" [attr.aria-invalid]="!!message()" (input)="value.set($any($event.target).value)" (keydown.enter)="commit.emit()" (blur)="leave()" />
          }
          @default {
            <input class="input num" type="number" inputmode="decimal" [id]="id" [value]="numberText()" [attr.min]="limits().min" [attr.max]="limits().max" [attr.step]="limits().step" [disabled]="disabled()" [attr.aria-describedby]="describedBy()" [attr.aria-invalid]="!!message()" (input)="value.set(number($any($event.target).value))" (keydown.enter)="commit.emit()" (blur)="leave()" />
            @if (unitLabel()) {
              <span class="unit">{{ unitLabel() }}</span>
            }
          }
        }
        @if (hint()) {
          <span class="hint" [id]="id + '-hint'">{{ hint() }}</span>
        }
      </div>
    }
    @if (f.description) {
      <p class="help" [id]="id + '-help'">{{ f.description }}</p>
    }
    @if (message(); as m) {
      <p class="error" [id]="id + '-error'" role="alert">{{ m }}</p>
    }
  `,
  styles: `
    :host { display: flex; flex-direction: column; gap: var(--pd-space-1); padding: var(--pd-space-3) 0; border-bottom: 1px solid var(--pd-hairline); min-width: 0; }
    .head { display: flex; align-items: center; gap: var(--pd-space-2); min-height: 28px; flex-wrap: wrap; }
    .label { font-weight: var(--pd-weight-medium); color: var(--pd-text); min-width: 0; overflow-wrap: anywhere; }
    .badge { flex: none; padding: 0 var(--pd-space-2); border-radius: var(--pd-radius-pill); border: 1px solid var(--pd-hairline); color: var(--pd-text-muted); font-size: var(--pd-text-caption); }
    .spacer { flex: 1; }
    .reset { min-height: 24px; padding: 0 var(--pd-space-2); border: 1px solid transparent; border-radius: var(--pd-radius-control); background: transparent; color: var(--pd-text-muted); font: inherit; font-size: var(--pd-text-caption); cursor: pointer; }
    .reset:hover { border-color: var(--pd-border-control); color: var(--pd-text); }
    .control { display: flex; align-items: center; gap: var(--pd-space-2); flex-wrap: wrap; min-width: 0; }
    .input { min-height: var(--pd-button-height); padding: 0 var(--pd-space-2); border-radius: var(--pd-radius-control); border: 1px solid var(--pd-border-control); background: var(--pd-surface-1); color: var(--pd-text); font: inherit; min-width: 0; max-width: 100%; box-sizing: border-box; }
    select.input { padding-inline-end: var(--pd-space-1); }
    .input.num { width: 9rem; }
    .input.wide { width: min(32rem, 100%); }
    textarea.input { padding: var(--pd-space-1) var(--pd-space-2); font-family: var(--pd-font-mono, ui-monospace, monospace); }
    .mono { font-family: var(--pd-font-mono, ui-monospace, monospace); }
    .switch { width: 18px; height: 18px; margin: 0; accent-color: var(--pd-accent); }
    .input:focus-visible, .switch:focus-visible, .reset:focus-visible { outline: 2px solid var(--pd-accent); outline-offset: 1px; }
    .unit { color: var(--pd-text-muted); }
    .hint { color: var(--pd-text-muted); font-size: var(--pd-text-caption); }
    .help { margin: 0; color: var(--pd-text-muted); font-size: var(--pd-text-caption); line-height: 1.45; }
    .error { margin: 0; color: var(--pd-danger-text, var(--pd-text)); font-size: var(--pd-text-caption); }
    :host(.invalid) .input { border-color: var(--pd-danger-text, var(--pd-border-control)); }
  `,
})
export class PdSettingField implements FormValueControl<SettingValue> {
  /** The field's schema (from the engine). */
  readonly setting = input.required<SettingField>();
  /** The value, kept in sync with the form by `[formField]`. */
  readonly value = model<SettingValue>('');
  /** The form's errors for this field: the engine's refusal or the pre-check. */
  readonly errors = input<readonly ValidationError.WithOptionalFieldTree[]>([]);
  readonly disabled = input(false);
  /** The value differs from the applied one. */
  readonly dirty = input(false);
  readonly touch = output<void>();
  /** Apply this edit now. */
  readonly commit = output<void>();
  readonly resetToDefault = output<void>();

  protected readonly id = `pd-setting-${++nextId}`;
  protected readonly unit = computed(() => editUnit(this.setting().kind));
  protected readonly unitLabel = computed(() => {
    const k = this.setting().kind;
    return this.unit()?.label ?? (k.type === 'int' || k.type === 'float' ? (k.unit ?? '') : '');
  });
  protected readonly limits = computed(() => {
    const k = this.setting().kind;
    const factor = this.unit()?.factor ?? 1;
    if (k.type === 'int' || k.type === 'float') return { min: k.min, max: k.max, step: k.step };
    if (k.type === 'duration') return { min: k.minMs / factor, max: k.maxMs / factor, step: 1 };
    if (k.type === 'size') return { min: k.minBytes / factor, max: k.maxBytes / factor, step: 1 };
    return { min: null, max: null, step: null };
  });
  protected readonly hint = computed(() => rangeHint(this.setting().kind));
  protected readonly isDefault = computed(() => sameValue(this.value(), this.setting().default));
  protected readonly defaultText = computed(() => displayValue(this.setting(), this.setting().default));
  /** The first error's message. */
  protected readonly message = computed(() => this.errors()[0]?.message ?? null);
  protected readonly describedBy = computed(() => {
    const ids: string[] = [];
    if (this.hint() && this.setting().kind.type !== 'bool') ids.push(`${this.id}-hint`);
    if (this.setting().description) ids.push(`${this.id}-help`);
    if (this.message()) ids.push(`${this.id}-error`);
    return ids.join(' ') || null;
  });
  protected readonly text = computed(() => {
    const v = this.value();
    return typeof v === 'string' ? v : String(v ?? '');
  });
  protected readonly listText = computed(() => {
    const v = this.value();
    return Array.isArray(v) ? v.join('\n') : String(v ?? '');
  });
  protected readonly numberText = computed(() => {
    const v = this.value();
    return typeof v === 'number' ? String(v / (this.unit()?.factor ?? 1)) : String(v ?? '');
  });

  /** A switch or a choice: the value, then apply at once. */
  protected choose(value: SettingValue): void {
    this.value.set(value);
    this.commit.emit();
  }

  /** Leaving a typed control: touched, and apply what was typed. */
  protected leave(): void {
    this.touch.emit();
    this.commit.emit();
  }

  protected number(text: string): SettingValue {
    return parseNumber(text, this.unit()?.factor ?? 1);
  }

  protected list(text: string): string[] {
    return parseList(text);
  }
}
