import { ChangeDetectionStrategy, Component, OnInit, computed, inject, signal } from '@angular/core';
import { AsyncState } from '../../shared/async-state';
import { formatBytes, formatMetric } from '../../shared/format';
import { Icon } from '../../shared/icon';
import { ExtrasStore } from '../../state/extras.store';
import { type UsageKey, type UsageSort, hue, initial, nextSort, sortUsage } from './extras-model';
import { EXTRAS_STYLES } from './extras-styles';
import { MatTooltip } from '@angular/material/tooltip';

/** Network traffic and energy impact per app since it started, sortable by any column. */
@Component({
  selector: 'dy-extras-apps',
  imports: [MatTooltip, AsyncState, Icon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <dy-async [loaded]="!!data()" [error]="store.apps().error" [empty]="data()?.apps?.length === 0" emptyTitle="No app activity to show" emptyText="Dyno has no network or energy numbers for any app yet." emptyIcon="network" what="app usage" [rows]="6" (retry)="store.loadApps()">
      @if (data(); as u) {
        @if (u.networkUnavailable; as why) { <p class="note" role="status"><dy-icon name="info" /><span><strong>Network traffic is not available:</strong> {{ why }}</span></p> }
        @if (u.energyUnavailable; as why) { <p class="note" role="status"><dy-icon name="info" /><span><strong>Energy impact is not available:</strong> {{ why }}</span></p> }
        <div class="tablewrap">
          <table class="table tbl" aria-label="Network and energy per app">
            <thead>
              <tr>
                <th [attr.aria-sort]="aria('app')"><button type="button" class="sort" (click)="sortBy('app')">App {{ arrow('app') }}</button></th>
                @if (!u.networkUnavailable) {
                  <th class="r w-num" [attr.aria-sort]="aria('bytesIn')"><button type="button" class="sort" (click)="sortBy('bytesIn')">Received {{ arrow('bytesIn') }}</button></th>
                  <th class="r w-num" [attr.aria-sort]="aria('bytesOut')"><button type="button" class="sort" (click)="sortBy('bytesOut')">Sent {{ arrow('bytesOut') }}</button></th>
                }
                @if (!u.energyUnavailable) {
                  <th class="r w-num" [attr.aria-sort]="aria('energyImpact')"><button type="button" class="sort" (click)="sortBy('energyImpact')">Energy impact {{ arrow('energyImpact') }}</button></th>
                }
              </tr>
            </thead>
            <tbody>
              @for (a of rows(); track a.app) {
                <tr>
                  <td><span class="tile" aria-hidden="true" [style.background]="tile(a.app)">{{ initial(a.app) }}</span><span class="nm" [matTooltip]="a.app">{{ a.app }}</span></td>
                  @if (!u.networkUnavailable) {
                    <td class="r">{{ bytes(a.bytesIn) }}</td>
                    <td class="r">{{ bytes(a.bytesOut) }}</td>
                  }
                  @if (!u.energyUnavailable) {
                    <td class="r">{{ metric(a.energyImpact) }}</td>
                  }
                </tr>
              }
            </tbody>
          </table>
        </div>
        <p class="faint muted">Traffic is counted since each app started. Energy impact is the system's own score right now; higher is more battery use.</p>
      }
    </dy-async>
  `,
  styles: [
    EXTRAS_STYLES,
    `
      .w-num { width: 150px; }
      .sort { all: unset; cursor: pointer; font: inherit; color: inherit; }
      .sort:hover, .sort:focus-visible { color: var(--text-1); }
      .sort:focus-visible { outline: 2px solid var(--focus); outline-offset: 2px; border-radius: 2px; }
      .nm { vertical-align: middle; }
    `,
  ],
})
export class ExtrasApps implements OnInit {
  protected readonly store = inject(ExtrasStore);
  protected readonly data = computed(() => this.store.apps().data);
  protected readonly sort = signal<UsageSort>({ key: 'bytesIn', dir: 'desc' });
  protected readonly rows = computed(() => sortUsage(this.data()?.apps ?? [], this.sort()));
  protected readonly initial = initial;
  protected readonly bytes = formatBytes;
  protected readonly metric = formatMetric;
  protected readonly tile = (name: string) => `hsl(${hue(name)} 42% 38%)`;

  ngOnInit(): void {
    void this.store.ensure('apps');
  }

  protected sortBy(key: UsageKey): void {
    this.sort.update((s) => nextSort(s, key));
  }

  protected aria(key: UsageKey): 'ascending' | 'descending' | 'none' {
    const s = this.sort();
    return s.key === key ? (s.dir === 'asc' ? 'ascending' : 'descending') : 'none';
  }

  protected arrow(key: UsageKey): string {
    const s = this.sort();
    return s.key === key ? (s.dir === 'asc' ? '↑' : '↓') : '';
  }
}
