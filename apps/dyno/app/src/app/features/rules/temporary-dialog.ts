import { DialogRef } from '@angular/cdk/dialog';
import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';
import { DialogFrame } from '../../shared/dialog';
import { formatDayTime } from '../../shared/format';
import { nextOccurrence } from './rules-model';

/** Asks for the time of day a temporary rule should end; closes with `HH:MM`, or `null` when cancelled. */
@Component({
  selector: 'dy-temporary-dialog',
  imports: [DialogFrame],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <dy-dialog title="End this rule at a time" subtitle="The rule gives everything back and switches itself off." icon="clock">
      <div class="field">
        <label for="tmp-time">Ends at</label>
        <input id="tmp-time" class="input" type="time" [value]="time()" (input)="time.set($any($event.target).value)" />
      </div>
      <p class="note">{{ note() }}</p>
      <ng-container footer>
        <button type="button" class="btn" (click)="ref.close(null)">Cancel</button>
        <button type="button" class="btn primary" [disabled]="!valid()" (click)="ref.close(time())">Set end time</button>
      </ng-container>
    </dy-dialog>
  `,
  styles: `
    :host { --dialog-w: 400px; }
    .field { display: grid; gap: var(--sp-1); }
    .note { margin: 0; color: var(--text-2); font-size: var(--fs-sm); }
  `,
})
export class TemporaryDialog {
  protected readonly ref = inject<DialogRef<string | null>>(DialogRef);
  protected readonly time = signal('18:00');
  private readonly at = computed(() => nextOccurrence(this.time(), Date.now()));
  protected readonly valid = computed(() => this.at() !== null);
  protected readonly note = computed(() => {
    const at = this.at();
    return at === null ? 'Enter a time of day.' : `The rule ends ${formatDayTime(at)}.`;
  });
}
