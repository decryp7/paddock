import { DIALOG_DATA, DialogRef } from '@angular/cdk/dialog';
import { ChangeDetectionStrategy, Component, inject, signal } from '@angular/core';
import type { DeleteMode } from '../../core/api.types';
import { DialogFrame } from '../../shared/dialog';
import { formatBytes, plural } from '../../shared/format';

export interface CleanupConfirmData {
  count: number;
  bytes: number;
  rows: { name: string; count: number; bytes: number }[];
  mode: DeleteMode;
}

/** The last question before anything moves: what, how much, and where it goes (the quarantine can be undone here). */
@Component({
  selector: 'dy-cleanup-confirm-dialog',
  imports: [DialogFrame],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <dy-dialog title="Move these items?" [subtitle]="plural(data.count, 'item') + ' · ' + bytes(data.bytes)" icon="cleanup">
      <ul class="rows" aria-label="What will be moved">
        @for (r of data.rows; track r.name) {
          <li><span class="name">{{ r.name }}</span><span class="n">{{ plural(r.count, 'item') }}</span><span class="n">{{ bytes(r.bytes) }}</span></li>
        }
      </ul>
      <div class="field">
        <span class="label" id="cc-where">Where they go</span>
        <div class="seg" role="radiogroup" aria-labelledby="cc-where">
          <button type="button" role="radio" [attr.aria-checked]="mode() === 'quarantine'" (click)="mode.set('quarantine')">Quarantine for 7 days</button>
          <button type="button" role="radio" [attr.aria-checked]="mode() === 'recycleBin'" (click)="mode.set('recycleBin')">Recycle Bin / Trash</button>
        </div>
      </div>
      <p class="note">
        {{ mode() === 'quarantine' ? 'Dyno keeps them for 7 days and you can put them back from this page or the activity log. Disk space is freed when the week is over.' : 'They go to the system Recycle Bin or Trash. Put them back from there; disk space is freed when you empty it.' }}
        Each item is checked again right before it moves; anything in use or changed since the scan is skipped and listed.
      </p>
      <ng-container footer>
        <button type="button" class="btn" (click)="ref.close(null)">Cancel</button>
        <button type="button" class="btn primary" (click)="ref.close(mode())">{{ mode() === 'quarantine' ? 'Move to quarantine' : 'Move to Recycle Bin / Trash' }}</button>
      </ng-container>
    </dy-dialog>
  `,
  styles: `
    :host { --dialog-w: 520px; }
    .rows { list-style: none; margin: 0; padding: 0; display: grid; gap: var(--sp-1); max-height: 220px; overflow: auto; }
    .rows li { display: grid; grid-template-columns: minmax(0, 1fr) auto auto; gap: var(--sp-4); font-size: var(--fs-sm); }
    .name { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .n { color: var(--text-2); font-variant-numeric: tabular-nums; text-align: right; }
    .note { margin: 0; color: var(--text-2); font-size: var(--fs-sm); }
  `,
})
export class CleanupConfirmDialog {
  protected readonly data = inject<CleanupConfirmData>(DIALOG_DATA);
  protected readonly ref = inject<DialogRef<DeleteMode | null>>(DialogRef);
  protected readonly mode = signal<DeleteMode>(this.data.mode);
  protected readonly plural = plural;
  protected readonly bytes = formatBytes;
}
