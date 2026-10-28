import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { DIALOG_INPUT, DIALOG_REF, type DialogRef } from '@decryp7/paddock-ui/shell/dialogs';
import type { ConfirmInput } from './contributions';

/** A decision: modal, fitted to its content, Cancel and one way forward (the shell's decision kind; an OS window). */
@Component({
  selector: 'sp-confirm-dialog',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <h2 class="title">{{ input.title }}</h2>
    <p class="text">{{ input.message }}</p>
    <div class="actions">
      <button type="button" class="btn" data-dialog-close (click)="ref.close()">Cancel</button>
      <button type="button" class="btn" [class.primary]="!input.danger" [class.danger-fill]="input.danger" data-default (click)="ref.close(true)">
        {{ input.confirmLabel }}
      </button>
    </div>
  `,
  styles: `
    .title { margin: 0 0 var(--pd-space-2); font-size: var(--pd-text-title); font-weight: var(--pd-weight-semibold); }
    .text { margin: 0 0 var(--pd-space-4); color: var(--pd-text-muted); }
    .actions { display: flex; justify-content: flex-end; gap: var(--pd-space-2); }
  `,
})
export class ConfirmDialog {
  protected readonly ref = inject<DialogRef<boolean>>(DIALOG_REF);
  protected readonly input = inject(DIALOG_INPUT) as unknown as ConfirmInput;
}
