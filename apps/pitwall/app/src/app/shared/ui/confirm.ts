import { Dialog, DialogRef, DIALOG_DATA } from '@angular/cdk/dialog';
import { ChangeDetectionStrategy, Component, Injectable, inject } from '@angular/core';
import { firstValueFrom } from 'rxjs';

export interface ConfirmOptions {
  title: string;
  message: string;
  confirm: string;
  danger?: boolean;
}

/** A modal yes/no (CDK dialog: focus trap, Escape closes, focus returns to the trigger). */
@Component({
  selector: 'pw-confirm-dialog',
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: { class: 'pw-dialog', role: 'alertdialog', 'aria-labelledby': 'pw-confirm-title', 'aria-describedby': 'pw-confirm-msg' },
  template: `
    <h2 id="pw-confirm-title">{{ data.title }}</h2>
    <p id="pw-confirm-msg">{{ data.message }}</p>
    <div class="row">
      <button type="button" class="btn" (click)="ref.close(false)">Cancel</button>
      <button type="button" class="btn" [class.danger-solid]="data.danger" [class.primary]="!data.danger" cdkFocusInitial (click)="ref.close(true)">{{ data.confirm }}</button>
    </div>
  `,
  styles: `
    :host {
      display: block;
      width: 400px;
      padding: var(--sp-5);
    }
    h2 {
      font-size: var(--fs-md);
      margin-bottom: var(--sp-2);
    }
    p {
      color: var(--text-2);
    }
    .row {
      display: flex;
      justify-content: flex-end;
      gap: var(--sp-2);
      margin-top: var(--sp-5);
    }
  `,
})
export class ConfirmDialog {
  protected readonly data = inject<ConfirmOptions>(DIALOG_DATA);
  protected readonly ref = inject<DialogRef<boolean>>(DialogRef);
}

@Injectable({ providedIn: 'root' })
export class Confirm {
  private readonly dialog = inject(Dialog);

  async ask(options: ConfirmOptions): Promise<boolean> {
    const ref = this.dialog.open<boolean, ConfirmOptions>(ConfirmDialog, { data: options, backdropClass: 'pw-backdrop', panelClass: 'pw-dialog-panel' });
    return (await firstValueFrom(ref.closed)) === true;
  }
}
