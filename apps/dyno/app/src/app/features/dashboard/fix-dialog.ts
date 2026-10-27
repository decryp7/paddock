import { DIALOG_DATA, DialogRef } from '@angular/cdk/dialog';
import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import type { HealthReason } from '../../core/api.types';
import { DialogFrame } from '../../shared/dialog';
import { Icon } from '../../shared/icon';

/** Explains a suggested fix before anything changes: what it does, exactly what changes, whether it needs admin, and that it can be undone. */
@Component({
  selector: 'dy-fix-dialog',
  imports: [DialogFrame, Icon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <dy-dialog [title]="reason.fix!.label" [subtitle]="reason.title" icon="health">
      <p class="msg">{{ reason.fix!.explanation }}</p>
      @if (reason.fix!.preview.length) {
        <div class="preview">
          <span class="eyebrow">What will change</span>
          <ul>
            @for (line of reason.fix!.preview; track $index) {
              <li class="dy-selectable">{{ line }}</li>
            }
          </ul>
        </div>
      }
      <div class="facts">
        @if (reason.fix!.needsAdmin) {
          <span class="badge" data-tone="warn"><dy-icon name="shield" /> Asks for an administrator the first time</span>
        }
        <span class="badge" [attr.data-tone]="reason.fix!.undoable ? 'ok' : 'muted'">
          <dy-icon name="undo" /> {{ reason.fix!.undoable ? 'Undoable from Activity' : 'Cannot be undone' }}
        </span>
      </div>
      <ng-container footer>
        <button type="button" class="btn" (click)="ref.close(false)">Not now</button>
        <button type="button" class="btn primary" (click)="ref.close(true)">{{ reason.fix!.label }}</button>
      </ng-container>
    </dy-dialog>
  `,
  styles: `
    .msg { color: var(--text-2); }
    .preview { display: grid; gap: var(--sp-1); padding: var(--sp-3); border: 1px solid var(--border); border-radius: var(--r-md); background: var(--surface-inset); }
    ul { margin: 0; padding-left: var(--sp-4); display: grid; gap: 2px; font-size: var(--fs-sm); }
    .facts { display: flex; gap: var(--sp-2); flex-wrap: wrap; }
  `,
})
export class FixDialog {
  protected readonly reason = inject<HealthReason>(DIALOG_DATA);
  protected readonly ref = inject<DialogRef<boolean>>(DialogRef);
}
