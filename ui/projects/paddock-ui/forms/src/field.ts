import { NgTemplateOutlet } from '@angular/common';
import { ChangeDetectionStrategy, Component, computed, Directive, DoCheck, ElementRef, forwardRef, inject, input, signal, type OnDestroy, type TemplateRef } from '@angular/core';
import { MatError, MatFormField, MatFormFieldControl, MatHint, MatLabel, MatSuffix } from '@angular/material/form-field';
import { PdStatusGlyph } from '@decryp7/paddock-ui/icons';
import { Subject } from 'rxjs';
import { fieldMessage, type PdFieldMessages, type PdFieldStateLike } from './messages';

let nextId = 0;

type NativeControl = HTMLInputElement | HTMLTextAreaElement | HTMLSelectElement;

/**
 * A labelled form field with its hint and its error (plan X21; UI rules 11, 18; design guide §8, forms): an outline
 * `mat-form-field` with the label always floated in the outline's notch (D1, rule 47), the hint under it (replaced by the
 * error while there is one, so the line that matters sits next to the field), and when the field has errors (after the user
 * left it, or on submit) one message with the alert icon at 14 px. Put the control inside and mark it `pdFieldControl`, which
 * gives it the id the label points to and the `aria-describedby` (the hint or the error) and `aria-invalid` of the field.
 * Works with Signal Forms (`[formField]`) and with plain controls.
 *
 * The control is projected from the host's template, where `mat-form-field` cannot find it by itself, so the field puts
 * a proxy of Material's control contract in its own template that reports the marked control's state.
 *
 * ```html
 * <pd-field label="Port" hint="1 to 65535" [state]="form.port()" [submitted]="submitted()">
 *   <input type="number" pdFieldControl [formField]="form.port" />
 * </pd-field>
 * ```
 */
@Component({
  selector: 'pd-field',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [MatFormField, MatLabel, MatHint, MatError, MatSuffix, NgTemplateOutlet, PdStatusGlyph, forwardRef(() => PdFieldProxy)],
  host: { class: 'pd-field', '[class.pd-field--invalid]': 'invalid()', '[class.pd-field--short]': 'size() === "short"', '[class.pd-field--plain]': '!hasControl()' },
  template: `
    <ng-template #body><ng-content /></ng-template>
    @if (hasControl()) {
      <mat-form-field appearance="outline" floatLabel="always" subscriptSizing="dynamic">
        <mat-label>{{ label() }}</mat-label>
        <ng-container pdFieldProxy />
        <ng-container *ngTemplateOutlet="body" />
        @if (suffix(); as tpl) {
          <span matIconSuffix class="pd-field__suffix"><ng-container *ngTemplateOutlet="tpl" /></span>
        }
        @if (message(); as m) {
          <mat-error [id]="errorId" data-testid="field-error"><pd-status-glyph status="error" [size]="14" /><span>{{ m }}</span></mat-error>
        }
        @if (hint() && !message()) {
          <mat-hint class="pd-field__hint" [id]="hintId">{{ hint() }}</mat-hint>
        }
      </mat-form-field>
    } @else {
      <!-- No text control to outline (a picker made of a button, say): the classic layout, the label above the content. -->
      <span class="pd-field__label" [id]="controlId + '-label'">{{ label() }}</span>
      <div role="group" [attr.aria-labelledby]="controlId + '-label'"><ng-container *ngTemplateOutlet="body" /></div>
      @if (message(); as m) {
        <p class="pd-field__error" [id]="errorId" data-testid="field-error"><pd-status-glyph status="error" [size]="14" /><span>{{ m }}</span></p>
      }
      @if (hint() && !message()) {
        <span class="pd-field__hint" [id]="hintId">{{ hint() }}</span>
      }
    }
  `,
  styles: `
    :host { display: block; min-width: 0; }
    :host(.pd-field--plain) { display: grid; gap: var(--pd-space-1); align-content: start; }
    mat-form-field { width: 100%; min-width: 0; }
    /* A short field narrows the control only: the error and the hint keep the whole column (an error of 20 characters must not wrap). */
    :host(.pd-field--short) ::ng-deep .mat-mdc-text-field-wrapper { max-width: 8rem; }
    /* The outline draws the border, the fill and the focus: whatever look the marked control brings (the classic control classes of
       controls.css, a dialog's own input style) gives way inside the field. Matched by the marker, not by class names. */
    :host ::ng-deep [pdFieldControl][pdFieldControl] { box-sizing: border-box; min-height: 0; padding: 0; border: 0; border-radius: 0; background: transparent; box-shadow: none; }
    :host ::ng-deep [pdFieldControl][pdFieldControl]:is(:hover, :focus, :focus-visible, :disabled, [aria-invalid]) { border: 0; outline: none; background: transparent; box-shadow: none; }
    /* A disabled field reads as disabled: a quieter fill under the outline. */
    :host ::ng-deep .mat-form-field-disabled .mdc-text-field--outlined { background: color-mix(in srgb, var(--pd-surface-2), var(--pd-text) 3%); border-radius: var(--pd-radius-input); }
    .pd-field__label { font-size: var(--pd-text-label); font-weight: var(--pd-weight-medium); color: var(--pd-text-muted); }
    .pd-field__hint { font-size: var(--pd-text-label); color: var(--pd-text-muted); }
    .pd-field__error { display: flex; align-items: flex-start; gap: var(--pd-space-1); margin: 0; font-size: var(--pd-text-label); color: var(--pd-danger-text, var(--pd-danger)); }
    mat-error { display: flex; align-items: flex-start; gap: var(--pd-space-1); }
    mat-error pd-status-glyph, .pd-field__error pd-status-glyph { margin-top: 1px; }
  `,
})
export class PdField implements DoCheck {
  /** The visible label (sentence case). */
  readonly label = input.required<string>();
  /** One line of help under the control. */
  readonly hint = input('');
  /** The field's state, `form.name()` of a Signal Forms form; null for a field without validation. */
  readonly state = input<PdFieldStateLike | null>(null);
  /** The words for error kinds, beating the defaults. */
  readonly messages = input<PdFieldMessages>({});
  /** `short` for numbers, ports and codes: the field's width suggests what goes in it (about 8rem). */
  readonly size = input<'default' | 'short'>('default');
  /** The form was submitted: show the errors of fields the user never touched. */
  readonly submitted = input(false);
  /** An error the host knows without a form state (a time the picker could not read), shown like a validation error. */
  readonly error = input<string | null>(null);
  /** A template drawn at the end of the outline (a picker's toggle button), which may use template variables of the host. */
  readonly suffix = input<TemplateRef<unknown> | null>(null);

  private readonly id = ++nextId;
  /** The id of the control, for the label's `for`. */
  readonly controlId = `pd-field-${this.id}`;
  protected readonly hintId = `${this.controlId}-hint`;
  protected readonly errorId = `${this.controlId}-error`;

  /** The message shown, or null. */
  readonly message = computed(() => this.error() ?? fieldMessage(this.state(), this.messages(), this.submitted()));
  /** Whether the control is marked invalid. */
  readonly invalid = computed(() => this.message() !== null);
  /** Whether the field is required (a mark after the label). */
  readonly required = computed(() => this.state()?.required?.() === true);

  /** What the control's `aria-describedby` names: the ids `mat-form-field` computed (the hint, or the error replacing it). */
  readonly describedBy = signal<string | null>(null);
  /** The control marked `pdFieldControl` inside, if any. */
  readonly control = signal<PdFieldControl | null>(null);
  /** Whether there is a control to outline. */
  protected readonly hasControl = computed(() => this.control() !== null);
  /** Fires when something `mat-form-field` shows about the control changed (focus, value, disabled, error). */
  readonly changes = new Subject<void>();
  private seen = '';

  /** The control's disabled and empty state change without a notification (a bound `disabled`, a value set by a form). */
  ngDoCheck(): void {
    const el = this.control()?.element;
    const now = el ? `${el.disabled}|${el.value === ''}|${this.invalid()}|${this.required()}` : '';
    if (now !== this.seen) {
      this.seen = now;
      this.changes.next();
    }
  }
}

/**
 * Marks the control inside a `pd-field`: its id, `aria-describedby` and `aria-invalid` come from the field, and it takes the
 * classes `mat-form-field` styles its input with.
 */
@Directive({
  selector: '[pdFieldControl]',
  host: {
    class: 'mat-mdc-input-element mat-mdc-form-field-input-control mdc-text-field__input',
    '[class.mat-mdc-form-field-textarea-control]': 'element.nodeName === "TEXTAREA"',
    '[class.mat-mdc-native-select-inline]': 'element.nodeName === "SELECT"',
    '[id]': 'field.controlId',
    '[attr.aria-describedby]': 'field.describedBy()',
    '[attr.aria-invalid]': 'field.invalid() ? "true" : null',
    '(focus)': 'focus(true)',
    '(blur)': 'focus(false)',
    '(input)': 'field.changes.next()',
  },
})
export class PdFieldControl implements OnDestroy {
  protected readonly field = inject(PdField);
  /** The native control. */
  readonly element = inject<ElementRef<NativeControl>>(ElementRef).nativeElement;
  /** Whether the control has focus. */
  readonly focused = signal(false);
  constructor() {
    this.field.control.set(this);
    this.field.changes.next();
  }

  ngOnDestroy(): void {
    if (this.field.control() === this) this.field.control.set(null);
  }

  protected focus(on: boolean): void {
    this.focused.set(on);
    this.field.changes.next();
  }
}

/** Material's control contract for a `pd-field`, answered from the marked control (see {@link PdField}). */
@Directive({
  selector: '[pdFieldProxy]',
  providers: [{ provide: MatFormFieldControl, useExisting: forwardRef(() => PdFieldProxy) }],
})
export class PdFieldProxy implements MatFormFieldControl<string> {
  private readonly field = inject(PdField);

  readonly stateChanges = this.field.changes;
  readonly ngControl = null;
  /** `mat-native-select` makes Material draw the arrow of a native select. */
  get controlType(): string {
    return this.field.control()?.element.nodeName === 'SELECT' ? 'mat-native-select' : 'mat-input';
  }
  readonly autofilled = false;
  readonly shouldLabelFloat = true;

  get id(): string {
    return this.field.controlId;
  }
  get value(): string {
    return this.field.control()?.element.value ?? '';
  }
  get placeholder(): string {
    return this.field.control()?.element.getAttribute('placeholder') ?? '';
  }
  get focused(): boolean {
    return this.field.control()?.focused() ?? false;
  }
  get empty(): boolean {
    return this.value === '';
  }
  get required(): boolean {
    return this.field.required();
  }
  get disabled(): boolean {
    return this.field.control()?.element.disabled ?? false;
  }
  get errorState(): boolean {
    return this.field.invalid();
  }

  setDescribedByIds(ids: string[]): void {
    this.field.describedBy.set(ids.length ? ids.join(' ') : null);
  }

  onContainerClick(): void {
    this.field.control()?.element.focus();
  }
}
