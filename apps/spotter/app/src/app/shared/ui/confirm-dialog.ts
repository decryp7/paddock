import { Dialog, DIALOG_DATA, DialogModule, DialogRef } from '@angular/cdk/dialog';
import { ChangeDetectionStrategy, Component, Injectable, inject } from '@angular/core';

export interface ConfirmData {
  title: string;
  message: string;
  confirmLabel: string;
  danger?: boolean;
}

/** The confirmation dialog (CDK dialog: focus trap, Escape, restores focus). */
@Component({
  selector: 'sp-confirm-dialog',
  imports: [DialogModule],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="dialog" role="alertdialog" aria-modal="true" [attr.aria-label]="data.title">
      <h2>{{ data.title }}</h2>
      <p>{{ data.message }}</p>
      <div class="row">
        <button type="button" class="btn" (click)="ref.close(false)" cdkFocusInitial>Cancel</button>
        <button type="button" class="btn" [class.primary]="!data.danger" [class.danger-fill]="data.danger" (click)="ref.close(true)">
          {{ data.confirmLabel }}
        </button>
      </div>
    </div>
  `,
  styles: `
    .dialog {
      display: grid;
      gap: var(--pd-space-3);
      width: min(420px, 90vw);
      padding: var(--pd-space-6);
      border: 1px solid var(--pd-hairline);
      border-radius: var(--pd-radius-dialog);
      background: var(--pd-surface-3);
      box-shadow: var(--pd-shadow-3);
    }
    h2 {
      font-size: var(--pd-text-title);
      font-weight: var(--pd-weight-semibold);
    }
    p {
      color: var(--pd-text-muted);
    }
    .row {
      display: flex;
      justify-content: flex-end;
      gap: var(--pd-space-2);
    }
  `,
})
export class ConfirmDialog {
  protected readonly data = inject<ConfirmData>(DIALOG_DATA);
  protected readonly ref = inject<DialogRef<boolean>>(DialogRef);
}

/** Asks the user to confirm; resolves false when dismissed. */
@Injectable({ providedIn: 'root' })
export class Confirm {
  private readonly dialog = inject(Dialog);

  async ask(data: ConfirmData): Promise<boolean> {
    const ref = this.dialog.open<boolean, ConfirmData>(ConfirmDialog, { data, ariaModal: true, backdropClass: 'sp-backdrop' });
    return (await new Promise<boolean | undefined>((resolve) => ref.closed.subscribe(resolve))) === true;
  }
}
