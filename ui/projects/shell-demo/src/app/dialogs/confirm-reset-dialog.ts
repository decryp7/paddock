import { ChangeDetectionStrategy, Component, signal } from '@angular/core';
import type { DialogAction, DialogContent } from '@decryp7/paddock-ui/shell/dialogs';

/** A decision: modal, fitted to its content, Cancel and one destructive way forward (ADR 0026 §2: declared actions). */
@Component({
  selector: 'sd-confirm-reset-dialog',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `<p class="text">The count goes back to zero and the log is emptied. This cannot be undone.</p>`,
  styleUrl: './dialog-content.css',
})
export class ConfirmResetDialog implements DialogContent<boolean> {
  readonly dialogHeading = signal('Reset the greetings?');
  readonly dialogActions = signal<readonly DialogAction<boolean>[]>([{ kind: 'answer', id: 'reset', label: 'Reset greetings', role: 'destructive', answer: () => true }]);
}
