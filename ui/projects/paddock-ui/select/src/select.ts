import { afterNextRender, ChangeDetectionStrategy, Component, computed, DOCUMENT, effect, inject, Injector, input, model, signal, viewChild } from '@angular/core';
import { ErrorStateMatcher, MatOption } from '@angular/material/core';
import { MatFormField, MatLabel } from '@angular/material/form-field';
import { MatSelect, type MatSelectChange } from '@angular/material/select';

/** One choice of a `pd-select`. */
export interface PdSelectOption {
  readonly value: string;
  readonly label: string;
  readonly disabled?: boolean;
}

let nextId = 0;

/**
 * A themed, non-native select (design guide §8, popovers): a `mat-select` in an outline `mat-form-field` (rule 47, Material
 * first), so the trigger and the list follow the theme, accent and density through the Material bridge (a native `<select>`
 * is drawn by the OS and ignores them). Keyboard, typeahead, the ARIA combobox and listbox roles, flipping at the window's
 * edge and Escape handling are Material's: Enter, Space or Down opens, Up/Down/Home/End/typed letters move, Enter chooses,
 * Escape or a click outside closes and returns focus to the trigger.
 *
 * ```html
 * <pd-select [options]="themes" [value]="theme()" ariaLabel="Theme" (valueChange)="theme.set($event)" />
 * ```
 *
 * A `label` input puts the label in the outline's notch. A `<label for>` that names the select through `inputId` is found in the document and wired as `aria-labelledby` (a
 * `<label for>` does not name a `combobox` element by itself).
 */
@Component({
  selector: 'pd-select',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [MatFormField, MatLabel, MatSelect, MatOption],
  host: { '[class.pd-invalid]': 'invalid()' },
  template: `
    <mat-form-field appearance="outline" floatLabel="always" subscriptSizing="dynamic">
      @if (label()) {
        <mat-label>{{ label() }}</mat-label>
      }
      <mat-select
        #select
        [id]="inputId() || id"
        [value]="chosen() ? value() : null"
        [placeholder]="placeholder()"
        [disabled]="disabled()"
        [errorStateMatcher]="matcher"
        [aria-label]="ariaLabel() || outsideLabel()"
        [aria-labelledby]="labelledBy()"
        [aria-describedby]="describedBy() || ''"
        (selectionChange)="pick($event)"
        (keydown.escape)="escape($event)"
      >
        @for (o of options(); track o.value) {
          <mat-option [value]="o.value" [disabled]="o.disabled === true">{{ o.label }}</mat-option>
        }
      </mat-select>
    </mat-form-field>
  `,
  styles: `
    :host { display: inline-flex; min-width: 0; max-width: 100%; }
    mat-form-field { width: 100%; min-width: 0; }
    :host ::ng-deep .mat-form-field-disabled .mdc-text-field--outlined { background: color-mix(in srgb, var(--pd-surface-2), var(--pd-text) 3%); border-radius: var(--pd-radius-input); }
    /* The keyboard ring is the family's: Material draws none on the select itself. */
    :host ::ng-deep .mat-mdc-select:focus-visible { outline: 2px solid var(--pd-focus-ring); outline-offset: 2px; }
  `,
})
export class PdSelect {
  /** The choices, in order. */
  readonly options = input.required<readonly PdSelectOption[]>();
  /** The chosen value; none of the options (or '') shows the placeholder. */
  readonly value = model<string>('');
  /** Text for no choice. */
  readonly placeholder = input<string>('');
  readonly disabled = input(false);
  readonly invalid = input(false);
  /** A visible label, in the outline's notch (D1); without it the select is named by `ariaLabel` or an outside `<label for>`. */
  readonly label = input<string>('');
  /** The accessible name when no `<label for>` names the trigger. */
  readonly ariaLabel = input<string>('');
  /** Space-separated ids of the hint and error texts. */
  readonly describedBy = input<string>('');
  /** The trigger's id, so a `<label for>` names it. */
  readonly inputId = input<string>('');

  protected readonly id = `pd-select-${++nextId}`;
  private readonly document = inject(DOCUMENT);
  private readonly injector = inject(Injector);
  private readonly select = viewChild.required(MatSelect);
  /** The outside `<label for>`: its id when it has one, else its text (an id is not ours to write onto it). */
  private readonly externalLabel = signal<{ id: string | null; text: string } | null>(null);

  /** Material shows the error state when this says so: the host's `invalid` input. */
  protected readonly matcher: ErrorStateMatcher = { isErrorState: () => this.invalid() };
  protected readonly chosen = computed(() => this.options().some((o) => o.value === this.value()));
  protected readonly labelledBy = computed(() => this.externalLabel()?.id ?? '');
  protected readonly outsideLabel = computed(() => (this.externalLabel()?.id ? '' : (this.externalLabel()?.text ?? '')));

  constructor() {
    // Material only checks the error state of a select that has a form control: this one is told by `invalid`.
    // effect-state: pushes the `invalid` input into Material's own error tracker, which has no signal input
    effect(() => {
      this.invalid();
      this.select().updateErrorState();
    });
    // effect-state: the outside label's id is a DOM lookup after render, which a computed cannot do
    effect(() => {
      const id = this.inputId();
      this.externalLabel.set(null);
      if (id) afterNextRender(() => this.externalLabel.set(this.labelFor(id)), { injector: this.injector });
    });
  }

  protected pick(e: MatSelectChange): void {
    const next = e.value as string;
    if (next !== this.value()) this.value.set(next);
  }

  /** Escape closes the open list and goes no further: the window or dialog around must not also close (Material closes the list from
   *  the page's keydown, which a stopped event would never reach, so the list is closed here). */
  protected escape(e: Event): void {
    const select = this.select();
    if (!select.panelOpen) return;
    e.stopPropagation();
    select.close();
  }

  /** The `<label for="id">` in the document, as its id or its text; null when there is none. */
  private labelFor(id: string): { id: string | null; text: string } | null {
    const label = [...this.document.querySelectorAll('label[for]')].find((l) => l.getAttribute('for') === id);
    return label ? { id: label.id || null, text: label.textContent?.trim() ?? '' } : null;
  }
}
