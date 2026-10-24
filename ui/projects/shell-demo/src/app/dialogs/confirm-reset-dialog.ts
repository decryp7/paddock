import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { DIALOG_REF, type DialogRef } from '@decryp7/paddock-ui/shell/dialogs';

/** A decision: modal, fitted to its content, Cancel and one way forward. */
@Component({
  selector: 'sd-confirm-reset-dialog',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <p class="text">The greeting count goes back to zero. This cannot be undone.</p>
    <div class="actions">
      <button type="button" class="btn" data-dialog-close (click)="ref.close()">Cancel</button>
      <button type="button" class="btn primary" data-default (click)="ref.close(true)">Reset</button>
    </div>
  `,
  styleUrl: './dialog-content.css',
})
export class ConfirmResetDialog {
  protected readonly ref = inject<DialogRef<boolean>>(DIALOG_REF);
}
