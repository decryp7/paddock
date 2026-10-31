import { ChangeDetectionStrategy, Component, computed, input } from '@angular/core';
import { answerOf, type ShellPorts } from '@decryp7/paddock-ui/shell';
import type { DialogAction, DialogContent } from '@decryp7/paddock-ui/shell/dialogs';

export interface ConfirmData {
  title: string;
  body: string;
  confirm: string;
  danger?: boolean;
}

/** A confirmation for actions that cannot be undone. The frame draws the question, the icon and the buttons. */
@Component({
  selector: 'pc-confirm-dialog',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `<p class="muted">{{ body() }}</p>`,
  styles: `
    p {
      margin: 0;
    }
  `,
})
export class ConfirmDialog implements DialogContent<boolean> {
  readonly title = input.required<string>();
  readonly body = input.required<string>();
  readonly confirm = input.required<string>();
  readonly danger = input(false);

  readonly dialogHeading = computed(() => this.title());
  readonly dialogActions = computed<readonly DialogAction<boolean>[]>(() => [
    { kind: 'dismiss', id: 'cancel', label: 'Cancel' },
    { kind: 'answer', id: 'confirm', label: this.confirm(), role: this.danger() ? 'destructive' : 'primary', answer: () => true },
  ]);
}

/** The dialog's id (`pc.confirm` in the contributions). */
export const CONFIRM_DIALOG_ID = 'pc.confirm';

/** Opens the confirmation as its own window (modal); resolves true when confirmed. */
export async function confirmAction(ports: Pick<ShellPorts, 'openDialog'>, data: ConfirmData): Promise<boolean> {
  return answerOf(await ports.openDialog(CONFIRM_DIALOG_ID, data)) === true;
}
