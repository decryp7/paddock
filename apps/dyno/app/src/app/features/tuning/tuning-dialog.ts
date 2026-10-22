import { DIALOG_DATA, DialogRef } from '@angular/cdk/dialog';
import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import type { TuningSuggestion } from '../../core/api.types';
import { DialogFrame } from '../../shared/dialog';

/** What the dialog is asked: apply a suggestion, or put the earlier setting back (optionally over a change made elsewhere). */
export interface TuningDialogData {
  suggestion: TuningSuggestion;
  mode: 'apply' | 'revert';
  /** Revert although something else changed the setting since. */
  force: boolean;
}

/** Shows the exact change and its exact inverse before anything is done. Resolves `true` only for the confirm button. */
@Component({
  selector: 'dy-tuning-dialog',
  imports: [DialogFrame],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <dy-dialog [title]="data.suggestion.title" [subtitle]="subtitle" icon="sparkle">
      @if (data.mode === 'revert' && data.force) {
        <p class="msg">Something else changed this setting after Dyno applied it. Reverting overwrites that newer setting with the earlier one.</p>
      } @else {
        <p class="msg">{{ data.suggestion.explanation }}</p>
      }
      <section class="lines" aria-label="What will change">
        <h3>{{ data.mode === 'apply' ? 'Dyno will' : 'Dyno will put back' }}</h3>
        <ul>
          @for (line of main(); track $index) {
            <li class="dy-selectable">{{ line }}</li>
          }
        </ul>
      </section>
      @if (data.mode === 'apply') {
        <section class="lines" aria-label="How to undo it">
          <h3>To undo it, Dyno will</h3>
          <ul>
            @for (line of data.suggestion.undoPreview; track $index) {
              <li class="dy-selectable">{{ line }}</li>
            }
          </ul>
        </section>
      }
      <ng-container footer>
        <button type="button" class="btn" (click)="ref.close(false)">Not now</button>
        <button type="button" class="btn primary" (click)="ref.close(true)">{{ data.mode === 'apply' ? 'Apply' : data.force ? 'Revert anyway' : 'Revert' }}</button>
      </ng-container>
    </dy-dialog>
  `,
  styles: `
    .msg { color: var(--text-2); }
    .lines { display: grid; gap: var(--sp-1); padding: var(--sp-3); border: 1px solid var(--border); border-radius: var(--r-md); background: var(--surface-inset); }
    h3 { font-size: var(--fs-sm); font-weight: var(--fw-medium); color: var(--text-2); }
    ul { margin: 0; padding-left: var(--sp-4); display: grid; gap: 2px; font-size: var(--fs-sm); }
  `,
})
export class TuningDialog {
  protected readonly data = inject<TuningDialogData>(DIALOG_DATA);
  protected readonly ref = inject<DialogRef<boolean>>(DialogRef);
  protected readonly subtitle = `${this.data.suggestion.current} → ${this.data.suggestion.proposed}`;
  protected readonly main = () => (this.data.mode === 'apply' ? this.data.suggestion.preview : this.data.suggestion.undoPreview);
}
