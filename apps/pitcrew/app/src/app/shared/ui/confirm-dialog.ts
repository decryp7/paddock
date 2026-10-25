import { DIALOG_DATA, Dialog, DialogRef } from '@angular/cdk/dialog';
import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { MatIcon } from '@angular/material/icon';
import { firstValueFrom } from 'rxjs';

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
        <span class="icon" [class.danger]="data.danger"><mat-icon [svgIcon]="data.danger ? 'pc:alert' : 'pc:info'" aria-hidden="true" /></span>
        <h2 id="confirm-title">{{ data.title }}</h2>
      </div>
      <p class="muted">{{ data.body }}</p>
      <div class="btn-row end">
        <button type="button" class="btn" (click)="ref.close(false)">Cancel</button>
        <button type="button" class="btn" [class.primary]="!data.danger" [class.danger]="data.danger" (click)="ref.close(true)">{{ data.confirm }}</button>
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
  protected readonly data = inject<ConfirmData>(DIALOG_DATA);
  protected readonly ref = inject<DialogRef<boolean>>(DialogRef);
}

/** Opens {@link ConfirmDialog}; resolves true when confirmed. */
export async function confirmAction(dialog: Dialog, data: ConfirmData): Promise<boolean> {
  const ref = dialog.open<boolean>(ConfirmDialog, {
    data,
    ariaLabelledBy: 'confirm-title',
    panelClass: 'pc-dialog-panel',
    backdropClass: 'pc-dialog-backdrop',
  });
  return (await firstValueFrom(ref.closed)) === true;
}
