import { ChangeDetectionStrategy, Component, OnInit, computed, inject } from '@angular/core';
import { PdTable, type PdColumn } from '@decryp7/paddock-ui/table';
import { AsyncState } from '../../shared/async-state';
import { plural } from '../../shared/format';
import { Icon } from '../../shared/icon';
import { ExtrasStore } from '../../state/extras.store';
import type { ReadVia, SleepBlocker, WakeEvent } from '../../core/api.types';
import { ReadingSource } from '../../core/reading-source';
import { type Numbered, PREVENTS_LABEL, heldForText, numberKey, numbered, wakeTimeText } from './extras-model';
import { EXTRAS_STYLES } from './extras-styles';

/** Why the computer will not sleep (what holds it awake and for how long) and what woke it recently. */
@Component({
  selector: 'dy-extras-sleep',
  imports: [AsyncState, Icon, PdTable, ReadingSource],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <dy-async [loaded]="!!data()" [error]="store.sleep().error" what="sleep information" [rows]="5" (retry)="store.loadSleep()">
      @if (data(); as s) {
        <h2 class="sec">Keeping this computer awake @if (!s.blockersUnavailable) { <span class="faint muted">{{ plural(s.blockers.length, 'program') }}</span> }</h2>
        <dy-reading-source [via]="s.blockersVia" [canRetry]="true" (retry)="store.loadSleep()" />
        @if (s.blockersUnavailable; as why) {
          <div class="none"><dy-icon name="info" /><strong>Could not read what keeps this computer awake</strong>@if (!needsHelper(s.blockersVia)) { <span>{{ why }}</span> }</div>
        } @else if (s.blockers.length === 0) {
          <div class="none"><dy-icon name="check" /><strong>Nothing is keeping this computer awake</strong><span>It can go to sleep when it is idle.</span></div>
        } @else {
          <pd-table [toolbar]="false" label="Keeping this computer awake" [rows]="blockerRows()" [columns]="blockerColumns" [rowKey]="rowNo" [filterable]="false" [maxHeight]="620" />
        }

        <h2 class="sec">Recent wakes @if (!s.wakesUnavailable) { <span class="faint muted">{{ plural(s.wakes.length, 'wake') }}</span> }</h2>
        @if (s.wakesUnavailable; as why) {
          <div class="none"><dy-icon name="info" /><strong>Could not read the recent wakes</strong><span>{{ why }}</span></div>
        } @else if (s.wakes.length === 0) {
          <div class="none"><dy-icon name="clock" /><strong>No wakes recorded</strong><span>The system log holds no wake events for the recent past.</span></div>
        } @else {
          <pd-table [toolbar]="false" label="Recent wakes" [rows]="wakeRows()" [columns]="wakeColumns" [rowKey]="rowNo" [filterable]="false" [maxHeight]="620" />
        }
      }
    </dy-async>
  `,
  styles: [EXTRAS_STYLES],
})
export class ExtrasSleep implements OnInit {
  protected readonly store = inject(ExtrasStore);
  protected readonly data = computed(() => this.store.sleep().data);
  protected readonly plural = plural;
  /** The reading source already says why and offers the button, so the empty state does not repeat the reason. */
  protected readonly needsHelper = (via: ReadVia): boolean => typeof via === 'object' && 'needsHelper' in via;
  protected readonly prevents = (p: keyof typeof PREVENTS_LABEL) => PREVENTS_LABEL[p];
  protected readonly held = heldForText;
  protected readonly when = wakeTimeText;
  /** Rows carry their place: the engine gives blockers and wakes no id. */
  protected readonly blockerRows = computed(() => numbered(this.data()?.blockers ?? []));
  protected readonly wakeRows = computed(() => numbered(this.data()?.wakes ?? []));
  protected readonly rowNo = numberKey;
  protected readonly blockerColumns: PdColumn<Numbered<SleepBlocker>>[] = [
    { key: 'name', header: 'Program', value: (b) => b.name, required: true, sortable: false, width: 'minmax(0, 1.4fr)' },
    { key: 'prevents', header: 'It prevents', value: (b) => PREVENTS_LABEL[b.prevents], sortable: false, width: '220px', hideBelow: 560 },
    { key: 'reason', header: 'Reason', value: (b) => b.reason, sortable: false, width: 'minmax(0, 2fr)', hideBelow: 700 },
    { key: 'held', header: 'Held for', value: (b) => b.heldFor, text: (b) => heldForText(b.heldFor), align: 'end', sortable: false, width: '110px' },
  ];
  protected readonly wakeColumns: PdColumn<Numbered<WakeEvent>>[] = [
    { key: 'when', header: 'When', value: (w) => w.when, text: (w) => wakeTimeText(w.when), required: true, sortable: false, width: '180px' },
    { key: 'reason', header: 'What woke it', value: (w) => w.reason, sortable: false, width: 'minmax(0, 1fr)' },
  ];

  ngOnInit(): void {
    void this.store.ensure('sleep');
  }
}
