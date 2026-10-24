import { ChangeDetectionStrategy, Component, computed, Directive, inject, input } from '@angular/core';
import { PdGlyph } from '@decryp7/paddock-ui/icons';
import { fieldMessage, type PdFieldMessages, type PdFieldStateLike } from './messages';

let nextId = 0;

/**
 * A labelled form field with its hint and its error (plan X21; UI rules 11, 18): the label above the control, the hint under
 * it (replaced by the error while there is one, so the line that matters sits next to the field), and when the field has errors (after the user left it, or on submit) one message with the alert icon at 14 px, announced
 * politely. Put the control inside and mark it `pdFieldControl`, which gives it the id the label points to and the
 * `aria-describedby` and `aria-invalid` of the field. Works with Signal Forms (`[formField]`) and with plain controls.
 *
 * ```html
 * <pd-field label="Port" hint="1 to 65535" [state]="form.port()" [submitted]="submitted()">
 *   <input class="pd-input" type="number" pdFieldControl [formField]="form.port" />
 * </pd-field>
 * ```
 */
@Component({
  selector: 'pd-field',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [PdGlyph],
  host: { class: 'pd-field', '[class.pd-field--invalid]': 'invalid()', '[class.pd-field--short]': 'size() === "short"' },
  template: `
    <label class="pd-field__label" [for]="controlId">{{ label() }}@if (required()) {<span class="pd-required" aria-hidden="true"> *</span>}</label>
    <ng-content />
    <div class="pd-messages" aria-live="polite">
      @if (message(); as m) {
        <p class="pd-field__error" [id]="errorId" data-testid="field-error"><pd-glyph name="alert" [size]="14" /><span>{{ m }}</span></p>
      }
    </div>
    @if (hint() && !message()) {
      <span class="pd-field__hint" [id]="hintId">{{ hint() }}</span>
    }
  `,
  styles: `
    :host { display: grid; gap: var(--pd-space-1); min-width: 0; align-content: start; }
    :host(.pd-field--short) { max-width: 8rem; }
    .pd-field__label { font-size: var(--pd-text-label); font-weight: var(--pd-weight-medium); color: var(--pd-text-muted); }
    .pd-required { color: var(--pd-danger-text, var(--pd-danger)); }
    .pd-field__hint { font-size: var(--pd-text-label); color: var(--pd-text-muted); }
    .pd-messages:empty { display: none; }
    .pd-field__error { display: flex; align-items: center; gap: var(--pd-space-1); margin: 0; font-size: var(--pd-text-label); color: var(--pd-danger-text, var(--pd-danger)); }
  `,
})
export class PdField {
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

  private readonly id = ++nextId;
  /** The id of the control, for the label's `for`. */
  readonly controlId = `pd-field-${this.id}`;
  protected readonly hintId = `${this.controlId}-hint`;
  protected readonly errorId = `${this.controlId}-error`;

  /** The message shown, or null. */
  readonly message = computed(() => fieldMessage(this.state(), this.messages(), this.submitted()));
  /** Whether the control is marked invalid. */
  readonly invalid = computed(() => this.message() !== null);
  /** Whether the field is required (a mark after the label). */
  protected readonly required = computed(() => this.state()?.required?.() === true);
  /** What the control's `aria-describedby` names: the error while there is one (it replaces the hint), else the hint. */
  readonly describedBy = computed(() => (this.message() ? this.errorId : this.hint() ? this.hintId : null));
}

/** Marks the control inside a `pd-field`: its id, `aria-describedby` and `aria-invalid` come from the field. */
@Directive({
  selector: '[pdFieldControl]',
  host: { '[id]': 'field.controlId', '[attr.aria-describedby]': 'field.describedBy()', '[attr.aria-invalid]': 'field.invalid() ? "true" : null' },
})
export class PdFieldControl {
  protected readonly field = inject(PdField);
}
