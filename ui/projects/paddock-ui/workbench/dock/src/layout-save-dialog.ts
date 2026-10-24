import { afterNextRender, ChangeDetectionStrategy, Component, ElementRef, inject, Injector, input, signal, viewChild } from '@angular/core';
import { FormField } from '@angular/forms/signals';
import { PdField, PdFieldControl } from '@decryp7/paddock-ui/forms';
import { DIALOG_REF, type DialogRef } from '@decryp7/paddock-ui/shell/dialogs';
import { layoutNameForm } from './layout-name-form';
import { cleanLayoutName } from './named-layouts';

/**
 * "Save layout" (a modal form, ADR 0009): the name of the layout the current arrangement is saved under. It validates the name
 * for empty, duplicate and too long (the names in use come in as input) and shows the error once the field was left or Save was
 * pressed. It calls nothing; it resolves the cleaned name, or nothing when cancelled. Enter saves, Escape cancels.
 */
@Component({
  selector: 'pd-layout-save-dialog',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [PdField, PdFieldControl, FormField],
  template: `
    <div class="pd-lsd">
      <pd-field [label]="t.label" [hint]="t.hint" [state]="f.name()" [submitted]="submitted()">
        <input #field class="pd-lsd__input" type="text" autocomplete="off" spellcheck="false" pdFieldControl [formField]="f.name" data-testid="layout-save-name" />
      </pd-field>
      <div class="pd-lsd__actions">
        <button type="button" class="pd-lsd__btn" data-dialog-close data-testid="layout-save-cancel" (click)="ref.close()">{{ t.cancel }}</button>
        <button type="button" class="pd-lsd__btn pd-lsd__btn--primary" data-default data-testid="layout-save-ok" (click)="save()">{{ t.save }}</button>
      </div>
    </div>
  `,
  styles: `
    :host { display: block; color: var(--pd-text); min-width: 0; }
    .pd-lsd { display: flex; flex-direction: column; gap: var(--pd-space-3); }
    .pd-lsd__input { box-sizing: border-box; width: 100%; min-height: var(--pd-control-height, 28px); padding: 0 var(--pd-space-2); border-radius: var(--pd-radius-control); border: 1px solid var(--pd-border-control); background: var(--pd-surface-1); color: var(--pd-text); font: inherit; }
    .pd-lsd__input:focus-visible { outline: 2px solid var(--pd-focus-ring); outline-offset: 1px; }
    .pd-lsd__input[aria-invalid='true'] { border-color: var(--pd-danger); }
    .pd-lsd__actions { display: flex; flex-wrap: wrap; justify-content: flex-end; gap: var(--pd-space-2); }
    .pd-lsd__btn { min-height: var(--pd-button-height, 28px); padding: 0 var(--pd-space-3); border-radius: var(--pd-radius-control); border: 1px solid var(--pd-border-control); background: var(--pd-surface-1); color: var(--pd-text); font: inherit; font-weight: var(--pd-weight-medium); cursor: pointer; }
    .pd-lsd__btn--primary { background: var(--pd-accent-fill); border-color: var(--pd-accent-fill); color: var(--pd-on-accent); }
    .pd-lsd__btn:focus-visible { outline: 2px solid var(--pd-focus-ring); outline-offset: 1px; }
  `,
})
export class PdLayoutSaveDialog {
  protected readonly ref = inject<DialogRef<string>>(DIALOG_REF);
  private readonly injector = inject(Injector);
  private readonly field = viewChild<ElementRef<HTMLInputElement>>('field');

  /** The names of the layouts that exist (a name may not repeat one). */
  readonly names = input<readonly string[]>([]);
  /** The name the field starts with ("Layout 3"). */
  readonly suggested = input('');

  protected readonly t = {
    label: $localize`:@@pd.wb.dock.layout.save-label:Layout name`,
    hint: $localize`:@@pd.wb.dock.layout.save-hint:Saves the windows and panels as they are now.`,
    save: $localize`:@@pd.wb.dock.layout.save-ok:Save`,
    cancel: $localize`:@@pd.wb.dock.layout.save-cancel:Cancel`,
  };
  protected readonly f = layoutNameForm(
    () => this.suggested(),
    () => this.names(),
  );
  protected readonly submitted = signal(false);

  constructor() {
    afterNextRender(() => this.field()?.nativeElement.select(), { injector: this.injector });
  }

  protected save(): void {
    this.submitted.set(true);
    const name = this.f.name();
    if (name.invalid()) {
      this.field()?.nativeElement.focus();
      return;
    }
    this.ref.close(cleanLayoutName(name.value()) ?? undefined);
  }
}
