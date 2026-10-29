import { ChangeDetectionStrategy, Component, DestroyRef, inject, signal } from '@angular/core';
import type { AlarmChange } from '../../core/api.types';
import { AsyncState } from '../../shared/async-state';
import { AlarmsStore } from '../../state/alarms.store';
import { TABLE_CSS } from './alarms-css';
import { changeLabel, shortTime } from './alarms-model';
import { FullTextTip } from '../../shared/full-text-tip';

/** The latest alarm changes (raised, acknowledged, shelved, ...), newest first. */
@Component({
  selector: 'dy-alarms-history',
  imports: [FullTextTip, AsyncState],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <dy-async [loaded]="store.historyLoaded()" [error]="store.historyError()" [empty]="store.history().length === 0" emptyTitle="No alarm history yet" emptyText="Every change to an alarm is listed here: raised, acknowledged, shelved and more." emptyIcon="clock" what="alarm history" [rows]="4" (retry)="store.loadHistory()">
      <p class="lead num">The last {{ store.history().length }} changes</p>
      <div class="grid" role="table" aria-label="Alarm history">
        <div class="head" role="row">
          <span role="columnheader">When</span>
          <span role="columnheader">Change</span>
          <span role="columnheader">Alarm</span>
          <span role="columnheader" class="c-rule">Rule</span>
        </div>
        @for (c of store.history(); track $index) {
          <div class="row" role="row">
            <span class="cell muted num" role="cell">{{ time(c.at) }}</span>
            <span class="cell" role="cell">{{ label(c) }}</span>
            <span class="cell" role="cell"><span class="truncate" [dyFullText]="c.alarm.title">{{ c.alarm.title }}</span></span>
            <span class="cell c-rule muted" role="cell"><span class="truncate id" [dyFullText]="c.alarm.rule">{{ c.alarm.rule }}</span></span>
          </div>
        }
      </div>
    </dy-async>
  `,
  styles: [
    TABLE_CSS,
    `
    :host { display: block; }
    dy-async { display: grid; gap: var(--sp-3); }
    .head, .row { --cols: 130px 170px minmax(180px, 1fr) minmax(140px, 220px); }
    .row { min-height: 36px; }
    @container (max-width: 700px) { .c-rule { display: none; } .head, .row { --cols: 130px 170px minmax(140px, 1fr); } }
  `,
  ],
})
export class AlarmsHistory {
  protected readonly store = inject(AlarmsStore);
  private readonly now = signal(Date.now());
  protected readonly label = (c: AlarmChange) => changeLabel(c);
  protected readonly time = (at: number) => shortTime(at, this.now());

  constructor() {
    if (!this.store.historyLoaded()) void this.store.loadHistory();
    const timer = setInterval(() => this.now.set(Date.now()), 30_000);
    inject(DestroyRef).onDestroy(() => clearInterval(timer));
  }
}
