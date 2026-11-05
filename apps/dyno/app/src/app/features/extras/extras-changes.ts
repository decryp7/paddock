import { ChangeDetectionStrategy, Component, OnInit, computed, inject, viewChild } from '@angular/core';
import { PdCell, PdTable, type PdColumn } from '@decryp7/paddock-ui/table';
import type { Change } from '../../core/api.types';
import { AsyncState } from '../../shared/async-state';
import { formatClock, plural } from '../../shared/format';
import { Icon } from '../../shared/icon';
import { ExtrasStore } from '../../state/extras.store';
import { CHANGE_KIND_LABEL, CHANGE_WINDOWS, changeRows, changeTone, watchingNote } from './extras-model';
import { EXTRAS_STYLES } from './extras-styles';
import { TreeActions } from '../../shared/tree-actions';
import { SuSegmentedControl, type SuSegment } from '@decryp7/paddock-ui';

interface ChangeLine extends Change {
  readonly key: string;
  readonly day: string;
}

/** What changed on this computer: installs, updates, new start-up items and setting changes, grouped by day. */
@Component({
  selector: 'dy-extras-changes',
  imports: [AsyncState, Icon, PdTable, PdCell, SuSegmentedControl, TreeActions],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="bar" role="group" aria-label="Window">
      <pd-segmented-control label="Period" [options]="windowSegs" [value]="store.hours() + ''" (valueChange)="store.setHours(+$event)" />
      <dy-tree-actions what="days" [disabled]="rows().length === 0" (expandAll)="gridRef()?.expandAllGroups()" (collapseAll)="gridRef()?.collapseAllGroups()" />
      <span class="grow"></span>
      @if (watching()) { <span class="faint muted">{{ watching() }}</span> }
    </div>
    <dy-async [loaded]="!!data()" [error]="state().error" what="changes" [rows]="5" (retry)="store.loadChanges()">
      @if (data(); as t) {
        @if (t.changes.length === 0) {
          <div class="none">
            <dy-icon name="clock" />
            <strong>Nothing changed in this period</strong>
            <span>No installs, updates, start-up items or setting changes were recorded. {{ watching() }}</span>
          </div>
        } @else {
          <pd-table #pdgrid [toolbar]="false" label="What changed" [rows]="rows()" [columns]="columns" [rowKey]="changeKey" [filterable]="false" height="window" groupBy="day">
            <ng-template pdCell="kind" let-c><span class="dot" [attr.data-tone]="tone(c.kind)" aria-hidden="true"></span>{{ kindLabel(c.kind) }}</ng-template>
          </pd-table>
        }
      }
    </dy-async>
  `,
  styles: [EXTRAS_STYLES],
})
export class ExtrasChanges implements OnInit {
  protected readonly store = inject(ExtrasStore);
  protected readonly windows = CHANGE_WINDOWS;
  protected readonly windowSegs: SuSegment[] = CHANGE_WINDOWS.map((w) => ({ value: String(w.hours), label: w.label }));
  protected readonly state = this.store.changes;
  protected readonly data = computed(() => this.store.changes().data);
  /** The changes, each with the day it belongs to (the model builds the day headings; the table groups by them). */
  protected readonly rows = computed<ChangeLine[]>(() => {
    const t = this.data();
    const out: ChangeLine[] = [];
    let day = '';
    for (const r of t ? changeRows(t.changes, t.to) : []) {
      if (r.kind === 'day') day = r.label;
      else out.push({ ...r.change, key: r.key, day });
    }
    return out;
  });
  protected readonly gridRef = viewChild<PdTable<ChangeLine>>('pdgrid');
  protected readonly changeKey = (c: ChangeLine): string => c.key;
  protected readonly columns: PdColumn<ChangeLine>[] = [
    { key: 'day', header: 'Day', value: (c) => c.day, hidden: true, sortable: false },
    { key: 'time', header: 'Time', value: (c) => c.at, text: (c) => formatClock(c.at), width: '72px', sortable: false },
    { key: 'kind', header: 'Kind', value: (c) => CHANGE_KIND_LABEL[c.kind], width: '180px', sortable: false },
    { key: 'title', header: 'Change', value: (c) => c.title, required: true, width: 'minmax(0, 1.4fr)', sortable: false },
    { key: 'detail', header: 'Details', value: (c) => c.detail, width: 'minmax(0, 2fr)', hideBelow: 640, sortable: false },
  ];
  protected readonly watching = computed(() => {
    const t = this.data();
    return t ? watchingNote(t.watchingSince, t.from, t.to) : '';
  });
  protected readonly plural = plural;
  protected readonly clock = formatClock;
  protected readonly kindLabel = (k: keyof typeof CHANGE_KIND_LABEL) => CHANGE_KIND_LABEL[k];
  protected readonly tone = changeTone;

  ngOnInit(): void {
    void this.store.ensure('changes');
  }
}
