import { ChangeDetectionStrategy, Component, computed, inject, input } from '@angular/core';
import { MatIcon } from '@angular/material/icon';
import type { ShellPorts } from '@decryp7/paddock-ui/shell';
import { DIALOG_REF, type DialogRef } from '@decryp7/paddock-ui/shell/dialogs';

export interface ConfirmData {
  title: string;
  body: string;
  confirm: string;
  danger?: boolean;
}

/** A confirmation for actions that cannot be undone. */
@Component({
  selector: 'pc-confirm-dialog',
  imports: [MatIcon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="wrap">
      <div class="head">
        <span class="icon" [class.danger]="data().danger"><mat-icon [svgIcon]="data().danger ? 'pc:alert' : 'pc:info'" aria-hidden="true" /></span>
        <h2 id="confirm-title">{{ data().title }}</h2>
      </div>
      <p class="muted">{{ data().body }}</p>
      <div class="btn-row end">
        <button type="button" class="btn" data-dialog-close (click)="ref.close()">Cancel</button>
        <button type="button" class="btn" [class.primary]="!data().danger" [class.danger]="data().danger" data-default (click)="ref.close(true)">{{ data().confirm }}</button>
      </div>
    </div>
  `,
  styles: `
    .wrap {
      display: grid;
      gap: var(--space-4);
      width: min(440px, calc(100vw - 48px));
      padding: var(--space-5);
    }
    .head {
      display: flex;
      align-items: center;
      gap: var(--space-3);
    }
    .icon {
      display: grid;
      place-items: center;
      width: 32px;
      height: 32px;
      border-radius: var(--radius-full);
      background: var(--info-soft);
      color: var(--info);
    }
    .icon.danger {
      background: var(--danger-soft);
      color: var(--danger);
    }
    .end {
      justify-content: flex-end;
    }
  `,
})
export class ConfirmDialog {
  readonly title = input.required<string>();
  readonly body = input.required<string>();
  readonly confirm = input.required<string>();
  readonly danger = input(false);
  protected readonly data = computed<ConfirmData>(() => ({ title: this.title(), body: this.body(), confirm: this.confirm(), danger: this.danger() }));
  protected readonly ref = inject<DialogRef<boolean>>(DIALOG_REF);
}

/** The dialog's id (`pc.confirm` in the contributions). */
export const CONFIRM_DIALOG_ID = 'pc.confirm';

/** Opens the confirmation as its own window (modal); resolves true when confirmed. */
export async function confirmAction(ports: Pick<ShellPorts, 'openDialog'>, data: ConfirmData): Promise<boolean> {
  return (await ports.openDialog(CONFIRM_DIALOG_ID, data)) === true;
}
