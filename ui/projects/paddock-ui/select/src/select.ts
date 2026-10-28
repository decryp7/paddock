import { afterNextRender, ChangeDetectionStrategy, Component, computed, DOCUMENT, effect, inject, Injector, input, model, signal, viewChild } from '@angular/core';
import { ErrorStateMatcher, MatOption } from '@angular/material/core';
import { MatFormField } from '@angular/material/form-field';
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
 * A `<label for>` that names the select through `inputId` is found in the document and wired as `aria-labelledby` (a
 * `<label for>` does not name a `combobox` element by itself).
 */
@Component({
  selector: 'pd-select',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [MatFormField, MatSelect, MatOption],
  host: { '[class.pd-invalid]': 'invalid()' },
  template: `
    <mat-form-field appearance="outline" floatLabel="always" subscriptSizing="dynamic">
      <mat-select
        #select
        hideSingleSelectionIndicator
        [id]="inputId() || id"
        [value]="chosen() ? value() : null"
        [placeholder]="placeholder()"
        [disabled]="disabled()"
        [errorStateMatcher]="matcher"
        [aria-label]="ariaLabel()"
        [aria-labelledby]="labelledBy()"
        [aria-describedby]="describedBy() || ''"
        (selectionChange)="pick($event)"
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
  private readonly externalLabel = signal<string | null>(null);

  /** Material shows the error state when this says so: the host's `invalid` input. */
  protected readonly matcher: ErrorStateMatcher = { isErrorState: () => this.invalid() };
  protected readonly chosen = computed(() => this.options().some((o) => o.value === this.value()));
  protected readonly labelledBy = computed(() => this.externalLabel() ?? '');

  constructor() {
    // Material only checks the error state of a select that has a form control: this one is told by `invalid`.
    effect(() => {
      this.invalid();
      this.select().updateErrorState();
    });
    effect(() => {
      const id = this.inputId();
      this.externalLabel.set(null);
      if (id) afterNextRender(() => this.externalLabel.set(this.labelIdFor(id)), { injector: this.injector });
    });
  }

  protected pick(e: MatSelectChange): void {
    const next = e.value as string;
    if (next !== this.value()) this.value.set(next);
  }

  /** The id of the `<label for="id">` in the document (given one if it has none), or null. */
  private labelIdFor(id: string): string | null {
    const label = [...this.document.querySelectorAll('label[for]')].find((l) => l.getAttribute('for') === id);
    if (!label) return null;
    if (!label.id) label.id = `${id}-label`;
    return label.id;
  }
}
