import { ChangeDetectionStrategy, Component, computed, inject, input, OnInit } from '@angular/core';
import { MatIcon } from '@angular/material/icon';
import { MatTooltip } from '@angular/material/tooltip';
import { DebriefStore } from '../../state/debrief.store';

/** "Open in Debrief" for a runner's log or one job of it. Always shown; without Debrief it says how to get one. */
@Component({
  selector: 'pc-debrief-button',
  imports: [MatIcon, MatTooltip],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <button
      type="button"
      class="btn ghost"
      [class.sm]="small()"
      [disabled]="store.busy()"
      [matTooltip]="tip()"
      [attr.aria-label]="'Open ' + label() + ' in Debrief'"
      (click)="store.open(runner(), label(), startedAt(), finishedAt())"
    >
      <mat-icon svgIcon="pc:external" aria-hidden="true" />{{ text() }}
    </button>
  `,
})
export class DebriefButton implements OnInit {
  readonly runner = input.required<string>();
  /** A short name for the file and the tooltip, e.g. `build #342`. */
  readonly label = input('log');
  readonly startedAt = input<number | null>(null);
  readonly finishedAt = input<number | null>(null);
  readonly small = input(true);
  readonly text = input('Debrief');

  protected readonly store = inject(DebriefStore);
  protected readonly tip = computed(() => {
    const s = this.store.status();
    if (s?.needsUpdate) return 'Update Debrief to use Open in Debrief. The log is still saved so you can open it by hand.';
    return s?.installed === false ? 'Debrief was not found. The log is still saved so you can open it by hand.' : 'Analyse this log in Debrief';
  });

  ngOnInit(): void {
    void this.store.load();
  }
}
