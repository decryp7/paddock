import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { DIALOG_REF, type DialogRef } from '@decryp7/paddock-ui/shell/dialogs';

/** A required choice: a modal without Cancel. Escape and a close request leave it open; it closes with its answer. */
@Component({
  selector: 'sd-choose-style-dialog',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <p class="text">How should the demo greet you from now on?</p>
    <div class="actions">
      <button type="button" class="btn" (click)="ref.close('casual')">Casual</button>
      <button type="button" class="btn primary" data-default (click)="ref.close('formal')">Formal</button>
    </div>
  `,
  styleUrl: './dialog-content.css',
})
export class ChooseStyleDialog {
  protected readonly ref = inject<DialogRef<'casual' | 'formal'>>(DIALOG_REF);
}
