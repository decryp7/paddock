import { ChangeDetectionStrategy, Component, DestroyRef, computed, inject, signal } from '@angular/core';
import { PdCell, PdTable, type PdColumn } from '@decryp7/paddock-ui/table';
import type { AlarmChange } from '../../core/api.types';
import { AsyncState } from '../../shared/async-state';
import { AlarmsStore } from '../../state/alarms.store';
import { changeLabel, shortTime, ruleLabel } from './alarms-model';

interface HistoryRow {
  readonly n: number;
  readonly change: AlarmChange;
}

/** The latest alarm changes (raised, acknowledged, shelved, ...), newest first. */
@Component({
  selector: 'dy-alarms-history',
  imports: [AsyncState, PdTable, PdCell],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <dy-async [loaded]="store.historyLoaded()" [error]="store.historyError()" [empty]="store.history().length === 0" emptyTitle="No alarm history yet" emptyText="Every change to an alarm is listed here: raised, acknowledged, shelved and more." emptyIcon="clock" what="alarm history" [rows]="4" (retry)="store.loadHistory()">
      <p class="lead num">The last {{ store.history().length }} changes</p>
      <pd-table [toolbar]="false" label="Alarm history" [rows]="rows()" [columns]="columns" [rowKey]="rowKey" [filterable]="false" [maxHeight]="560">
        <ng-template pdCell="rule" let-r><span class="id">{{ ruleName(r.change.alarm.rule) }}</span></ng-template>
      </pd-table>
    </dy-async>
  `,
  styles: `
    :host { display: block; }
    dy-async { display: grid; gap: var(--sp-3); }
    .id { font-size: var(--fs-sm); line-height: 1.2; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--text-2); }
  `,
})
export class AlarmsHistory {
  protected readonly store = inject(AlarmsStore);
  protected readonly ruleName = ruleLabel;
  private readonly now = signal(Date.now());
  protected readonly label = (c: AlarmChange) => changeLabel(c);
  protected readonly time = (at: number) => shortTime(at, this.now());
  /** The changes, each with its place (a change has no id of its own). */
  protected readonly rows = computed(() => this.store.history().map((change, n) => ({ n, change })));
  protected readonly rowKey = (r: HistoryRow): string => String(r.n);
  protected readonly columns: PdColumn<HistoryRow>[] = [
    { key: 'at', header: 'When', value: (r) => r.change.at, text: (r) => this.time(r.change.at), width: '130px' },
    { key: 'change', header: 'Change', value: (r) => this.label(r.change), width: '170px' },
    { key: 'alarm', header: 'Alarm', value: (r) => r.change.alarm.title, required: true, width: 'minmax(0, 1fr)' },
    { key: 'rule', header: 'Rule', value: (r) => ruleLabel(r.change.alarm.rule), width: 'minmax(140px, 220px)', hideBelow: 700 },
  ];

  constructor() {
    if (!this.store.historyLoaded()) void this.store.loadHistory();
    const timer = setInterval(() => this.now.set(Date.now()), 30_000);
    inject(DestroyRef).onDestroy(() => clearInterval(timer));
  }
}
