import { ChangeDetectionStrategy, Component, computed, ElementRef, inject, input, signal, viewChild } from '@angular/core';
import { FormField } from '@angular/forms/signals';
import { PdField, PdFieldControl } from '@decryp7/paddock-ui/forms';
import { DIALOG_REF, type DialogAction, type DialogContent, type DialogRef } from '@decryp7/paddock-ui/shell/dialogs';
import { layoutNameForm } from './layout-name-form';
import { cleanLayoutName } from './named-layouts';

/**
 * "Save layout" (a modal form, ADR 0009): the name of the layout the current arrangement is saved under. It validates the name
 * for empty, duplicate and too long (the names in use come in as input) and shows the error once the field was left or Save was
 * pressed. It calls nothing; it answers the cleaned name, or is dismissed. Its actions are declared (ADR 0026 §2): Save (the
 * primary, Enter) checks the name first and keeps the dialog open on an error; Cancel is the frame's.
 */
@Component({
  selector: 'pd-layout-save-dialog',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [PdField, PdFieldControl, FormField],
  template: `
    <div class="pd-lsd">
      <pd-field [label]="t.label" [hint]="t.hint" [state]="f.name()" [submitted]="submitted()">
        <input #field class="pd-lsd__input" type="text" autocomplete="off" spellcheck="false" data-autofocus pdFieldControl [formField]="f.name" data-testid="layout-save-name" (focus)="selectOnce($event)" />
      </pd-field>
    </div>
  `,
  styles: `
    :host { display: block; color: var(--pd-text); min-width: 0; }
    .pd-lsd { display: flex; flex-direction: column; gap: var(--pd-space-3); }
  `,
})
export class PdLayoutSaveDialog implements DialogContent<string> {
  private readonly ref = inject<DialogRef<string>>(DIALOG_REF);
  private readonly field = viewChild<ElementRef<HTMLInputElement>>('field');

  /** The names of the layouts that exist (a name may not repeat one). */
  readonly names = input<readonly string[]>([]);
  /** The name the field starts with ("Layout 3"). */
  readonly suggested = input('');

  protected readonly t = {
    label: $localize`:@@pd.wb.dock.layout.save-label:Layout name`,
    hint: $localize`:@@pd.wb.dock.layout.save-hint:Saves the windows and panels as they are now.`,
    save: $localize`:@@pd.wb.dock.layout.save-ok:Save`,
  };
  protected readonly f = layoutNameForm(
    () => this.suggested(),
    () => this.names(),
  );
  protected readonly submitted = signal(false);
  private selected = false;

  /** Save checks the name and answers it; an invalid name shows its error and keeps the dialog open. */
  readonly dialogActions = computed<readonly DialogAction<string>[]>(() => [{ kind: 'run', id: 'save', label: this.t.save, role: 'primary', run: () => this.save() }]);

  /** The suggested name is selected when the field first gets the focus, so typing replaces it. */
  protected selectOnce(e: FocusEvent): void {
    if (this.selected) return;
    this.selected = true;
    (e.target as HTMLInputElement).select();
  }

  private save(): void {
    this.submitted.set(true);
    const name = this.f.name();
    if (name.invalid()) {
      this.field()?.nativeElement.focus();
      return;
    }
    const clean = cleanLayoutName(name.value());
    if (clean) this.ref.answer(clean);
  }
}
