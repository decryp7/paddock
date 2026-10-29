import { ChangeDetectionStrategy, Component, OnInit, computed, inject, signal } from '@angular/core';
import { PdCell, PdTable, type PdColumn, type PdSort } from '@decryp7/paddock-ui/table';
import type { AppUsageRow } from '../../core/api.types';
import { AsyncState } from '../../shared/async-state';
import { formatBytes, formatMetric } from '../../shared/format';
import { Icon } from '../../shared/icon';
import { ExtrasStore } from '../../state/extras.store';
import { hue, initial } from './extras-model';
import { EXTRAS_STYLES } from './extras-styles';

/** Network traffic and energy impact per app since it started, sortable by any column. */
@Component({
  selector: 'dy-extras-apps',
  imports: [AsyncState, Icon, PdTable, PdCell],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <dy-async [loaded]="!!data()" [error]="store.apps().error" [empty]="data()?.apps?.length === 0" emptyTitle="No app activity to show" emptyText="Dyno has no network or energy numbers for any app yet." emptyIcon="network" what="app usage" [rows]="6" (retry)="store.loadApps()">
      @if (data(); as u) {
        @if (u.networkUnavailable; as why) { <p class="note" role="status"><dy-icon name="info" /><span><strong>Network traffic is not available:</strong> {{ why }}</span></p> }
        @if (u.energyUnavailable; as why) { <p class="note" role="status"><dy-icon name="info" /><span><strong>Energy impact is not available:</strong> {{ why }}</span></p> }
        <pd-table [toolbar]="false" label="Network and energy per app" [rows]="rows()" [columns]="columns()" [rowKey]="appKey" [filterable]="false" [maxHeight]="620" [(sort)]="sort">
          <ng-template pdCell="app" let-a><span class="app"><span class="tile" aria-hidden="true" [style.background]="tile(a.app)">{{ initial(a.app) }}</span><span class="nm">{{ a.app }}</span></span></ng-template>
        </pd-table>
        <p class="faint muted">Traffic is counted since each app started. Energy impact is the system's own score right now; higher is more battery use.</p>
      }
    </dy-async>
  `,
  styles: [
    EXTRAS_STYLES,
    `
      .app { display: flex; align-items: center; gap: var(--sp-2); min-width: 0; }
      .app .tile { flex: none; margin: 0; }
      .nm { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; min-width: 0; }
    `,
  ],
})
export class ExtrasApps implements OnInit {
  protected readonly store = inject(ExtrasStore);
  protected readonly data = computed(() => this.store.apps().data);
  /** The most received first, until the person chooses another order. */
  protected readonly sort = signal<PdSort | null>({ key: 'bytesIn', direction: 'desc' });
  protected readonly rows = computed(() => this.data()?.apps ?? []);
  protected readonly appKey = (a: AppUsageRow): string => a.app;
  /** The columns follow what the machine can measure. */
  protected readonly columns = computed<PdColumn<AppUsageRow>[]>(() => {
    const u = this.data();
    return [
      { key: 'app', header: 'App', value: (a: AppUsageRow) => a.app, required: true, width: 'minmax(0, 1fr)' },
      ...(u?.networkUnavailable
        ? []
        : [
            { key: 'bytesIn', header: 'Received', value: (a: AppUsageRow) => a.bytesIn, text: (a: AppUsageRow) => formatBytes(a.bytesIn), align: 'end' as const, width: '150px' },
            { key: 'bytesOut', header: 'Sent', value: (a: AppUsageRow) => a.bytesOut, text: (a: AppUsageRow) => formatBytes(a.bytesOut), align: 'end' as const, width: '150px' },
          ]),
      ...(u?.energyUnavailable ? [] : [{ key: 'energyImpact', header: 'Energy impact', value: (a: AppUsageRow) => a.energyImpact, text: (a: AppUsageRow) => formatMetric(a.energyImpact), align: 'end' as const, width: '150px' }]),
    ];
  });
  protected readonly initial = initial;
  protected readonly bytes = formatBytes;
  protected readonly metric = formatMetric;
  protected readonly tile = (name: string) => `hsl(${hue(name)} 42% 38%)`;

  ngOnInit(): void {
    void this.store.ensure('apps');
  }
}
