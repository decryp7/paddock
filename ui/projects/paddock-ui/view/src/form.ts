import { ChangeDetectionStrategy, Component, computed, inject, input, linkedSignal, signal } from '@angular/core';
import { PdField, PdFieldControl, type PdFieldError, type PdFieldStateLike } from '@decryp7/paddock-ui/forms';
import { PdGlyph } from '@decryp7/paddock-ui/icons';
import type { Field, FieldKind, Form } from '../generated/view';
import { PdViewButton } from './button';
import { ViewRuntime } from './runtime';

/** The values a form starts with: each field's own value (a toggle `false`, a missing path or secret `null`). */
export function initialValues(form: Form): Record<string, unknown> {
  const out: Record<string, unknown> = {};
  for (const f of form.fields) {
    const k = f.kind;
    out[f.id] = 'text' in k ? k.text.value : 'number' in k ? k.number.value : 'choice' in k ? k.choice.value : 'toggle' in k ? k.toggle.value : 'path' in k ? k.path.value : k.secretRef.value;
  }
  return out;
}

/** What is wrong with a field's value, or null: required fields need a value; numbers need to be numbers inside min and max. */
export function fieldProblem(field: Field, value: unknown): PdFieldError | null {
  const k: FieldKind = field.kind;
  const empty = value === null || value === undefined || value === '' || (typeof value === 'number' && Number.isNaN(value));
  if (field.required && !('toggle' in k) && empty) return { kind: 'required' };
  if ('number' in k && !empty) {
    const n = Number(value);
    if (!Number.isFinite(n)) return { kind: 'invalid' };
    if (k.number.min !== null && n < k.number.min) return { kind: 'min', min: k.number.min } as PdFieldError;
    if (k.number.max !== null && n > k.number.max) return { kind: 'max', max: k.number.max } as PdFieldError;
  }
  return null;
}

/**
 * A `Form` node: text, number, choice, toggle, path and secret-reference fields built from the plugin's description, each labelled
 * with its help text under it. The form never submits by itself: the submit button sends the action with `values` (field id to
 * value), after the fields passed their checks (the engine re-validates). A path field holds a `GrantRef` chosen with the host's
 * picker and a secret field a `SecretRef` chosen with the host's secret picker; the view never shows or types either.
 */
@Component({
  selector: 'pd-view-form',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [PdField, PdFieldControl, PdGlyph, PdViewButton],
  template: `
    <form class="pd-view-form" (submit)="submit($event)" novalidate>
      @for (f of form().fields; track f.id) {
        @if (f.kind; as k) {
          @if ('toggle' in k) {
            <div class="pd-view-form__check">
              <label class="pd-check">
                <input type="checkbox" class="pd-switch" role="switch" [checked]="values()[f.id] === true" [disabled]="runtime.disabled()" [attr.aria-describedby]="f.help ? helpId(f) : null" (change)="set(f.id, $any($event.target).checked)" />
                <span>{{ f.label }}</span>
              </label>
              @if (f.help) {
                <span class="pd-view-form__help" [id]="helpId(f)">{{ f.help }}</span>
              }
            </div>
          } @else {
            <pd-field [label]="f.label" [hint]="f.help" [state]="states().get(f.id) ?? null" [submitted]="submitted()" [size]="'number' in k ? 'short' : 'default'">
              @if ('text' in k) {
                @if (k.text.multiline) {
                  <textarea class="pd-textarea" pdFieldControl rows="3" [value]="str(f.id)" [placeholder]="k.text.placeholder" [disabled]="runtime.disabled()" (input)="set(f.id, $any($event.target).value)" (blur)="touch(f.id)"></textarea>
                } @else {
                  <input class="pd-input" pdFieldControl type="text" [value]="str(f.id)" [placeholder]="k.text.placeholder" [disabled]="runtime.disabled()" (input)="set(f.id, $any($event.target).value)" (blur)="touch(f.id)" />
                }
              } @else if ('number' in k) {
                <input class="pd-input" pdFieldControl type="number" [value]="str(f.id)" [attr.min]="k.number.min" [attr.max]="k.number.max" [disabled]="runtime.disabled()" (input)="setNumber(f.id, $any($event.target).value)" (blur)="touch(f.id)" />
              } @else if ('choice' in k) {
                <select class="pd-select" pdFieldControl [disabled]="runtime.disabled()" (change)="set(f.id, $any($event.target).value)" (blur)="touch(f.id)">
                  @if (!f.required || !str(f.id)) {
                    <option value="" [selected]="!str(f.id)">{{ none }}</option>
                  }
                  @for (o of k.choice.options; track o.value) {
                    <option [value]="o.value" [selected]="o.value === str(f.id)">{{ o.label }}</option>
                  }
                </select>
              } @else if ('path' in k) {
                <div class="pd-view-form__pick">
                  <span class="pd-view-form__chosen" [class.pd-view-form__chosen--none]="!values()[f.id]">{{ values()[f.id] ? chosen : (k.path.folder ? noFolder : noFile) }}</span>
                  <button type="button" class="pd-btn" [attr.aria-label]="f.label + ': ' + (k.path.folder ? chooseFolder : chooseFile)" [disabled]="runtime.disabled()" (click)="pickPath(f, k.path.folder)">{{ k.path.folder ? chooseFolder : chooseFile }}</button>
                </div>
              } @else if ('secretRef' in k) {
                <div class="pd-view-form__pick">
                  <span class="pd-view-form__chosen" [class.pd-view-form__chosen--none]="!values()[f.id]"><pd-glyph name="lock" [size]="14" />{{ values()[f.id] ? chosenSecret : noSecret }}</span>
                  <button type="button" class="pd-btn" [attr.aria-label]="f.label + ': ' + chooseSecret" [disabled]="runtime.disabled()" (click)="pickSecret(f)">{{ chooseSecret }}</button>
                </div>
              }
            </pd-field>
          }
        }
      }
      <div class="pd-view-form__actions">
        <pd-view-button [action]="form().submit" variant="primary" [extra]="{ values: values() }" [guard]="canSubmit" />
      </div>
    </form>
  `,
  styles: `
    :host { display: block; min-width: 0; max-width: var(--pd-form-max, 560px); }
    .pd-view-form { display: grid; gap: var(--pd-space-3); }
    .pd-view-form__check { display: grid; gap: var(--pd-space-1); }
    .pd-view-form__help { font-size: var(--pd-text-label); color: var(--pd-text-muted); }
    .pd-view-form__pick { display: flex; align-items: center; gap: var(--pd-space-2); flex-wrap: wrap; min-width: 0; }
    .pd-view-form__chosen { display: inline-flex; align-items: center; gap: var(--pd-space-1); min-width: 0; flex: 1; overflow-wrap: anywhere; }
    .pd-view-form__chosen--none { color: var(--pd-text-muted); }
    .pd-view-form__actions { display: flex; gap: var(--pd-space-2); }
  `,
})
export class PdViewForm {
  protected readonly runtime = inject(ViewRuntime);
  readonly form = input.required<Form>();
  /** The values being edited; they start again when the plugin sends a different form. */
  protected readonly values = linkedSignal<Form, Record<string, unknown>>({ source: this.form, computation: initialValues });
  private readonly touched = signal<ReadonlySet<string>>(new Set());
  protected readonly submitted = signal(false);
  protected readonly none = $localize`:@@pd.view.form.none:Not chosen`;
  protected readonly chosen = $localize`:@@pd.view.form.path-chosen:Chosen`;
  protected readonly noFile = $localize`:@@pd.view.form.no-file:No file chosen`;
  protected readonly noFolder = $localize`:@@pd.view.form.no-folder:No folder chosen`;
  protected readonly chooseFile = $localize`:@@pd.view.form.choose-file:Choose a file…`;
  protected readonly chooseFolder = $localize`:@@pd.view.form.choose-folder:Choose a folder…`;
  protected readonly chosenSecret = $localize`:@@pd.view.form.secret-chosen:A stored secret is chosen`;
  protected readonly noSecret = $localize`:@@pd.view.form.no-secret:No secret chosen`;
  protected readonly chooseSecret = $localize`:@@pd.view.form.choose-secret:Choose a secret…`;

  private readonly problems = computed(() => {
    const out = new Map<string, PdFieldError>();
    const v = this.values();
    for (const f of this.form().fields) {
      const p = fieldProblem(f, v[f.id]);
      if (p) out.set(f.id, p);
    }
    return out;
  });
  /** One state object per field, rebuilt only when a value or a touch changes (an input that changes identity every pass would fail the dev-mode check). */
  protected readonly states = computed(() => {
    const touched = this.touched();
    const problems = this.problems();
    return new Map<string, PdFieldStateLike>(
      this.form().fields.map((f) => {
        const p = problems.get(f.id);
        return [f.id, { errors: () => (p ? [p] : []), touched: () => touched.has(f.id), required: () => f.required }];
      }),
    );
  });
  /** The submit button's check: a form with a problem shows every error and sends nothing. */
  protected readonly canSubmit = (): boolean => {
    this.submitted.set(true);
    return this.problems().size === 0;
  };
  protected helpId(f: Field): string {
    return `pd-view-form-help-${f.id}`;
  }
  protected str(id: string): string {
    const v = this.values()[id];
    return v === null || v === undefined ? '' : String(v);
  }
  protected set(id: string, value: unknown): void {
    this.values.update((v) => ({ ...v, [id]: value }));
  }
  protected setNumber(id: string, text: string): void {
    this.set(id, text.trim() === '' ? null : Number(text));
  }
  protected touch(id: string): void {
    this.touched.update((t) => new Set(t).add(id));
  }
  /** Enter in a text field never submits by itself: only the submit button sends the action. */
  protected submit(e: Event): void {
    e.preventDefault();
  }
  protected async pickPath(f: Field, folder: boolean): Promise<void> {
    const grant = await this.runtime.pickPath(f.id, f.label, folder);
    if (grant) this.set(f.id, grant);
  }
  protected async pickSecret(f: Field): Promise<void> {
    const ref = await this.runtime.pickSecret(f.id, f.label);
    if (ref) this.set(f.id, ref);
  }
}
