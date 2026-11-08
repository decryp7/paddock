import { ChangeDetectionStrategy, Component, OnInit, computed, inject } from '@angular/core';
import { PdTable, type PdColumn } from '@decryp7/paddock-ui/table';
import type { BootStep } from '../../core/api.types';
import { ThemeService } from '../../core/theme';
import { AsyncState } from '../../shared/async-state';
import { Chart } from '../../shared/chart';
import { formatDayTime, formatSeconds } from '../../shared/format';
import { Icon } from '../../shared/icon';
import { ExtrasStore } from '../../state/extras.store';
import { bootOption } from './extras-model';
import { EXTRAS_STYLES } from './extras-styles';

/** How long the last start-up took, how recent boots compare, and what slowed the last one. */
@Component({
  selector: 'dy-extras-boot',
  imports: [AsyncState, Chart, Icon, PdTable],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <dy-async [loaded]="!!data()" [error]="store.boot().error" what="start-up information" [rows]="5" (retry)="store.loadBoot()">
      @if (data(); as b) {
        @if (b.boots.length === 0 && b.lastTotalSecs === null) {
          <div class="none"><dy-icon name="power" /><strong>No start-up history yet</strong><span>{{ b.stepsUnavailable ?? 'The system has not kept a record of recent boots that Dyno can read.' }}</span></div>
        } @else {
          <section class="lead" aria-label="Last start-up">
            <div class="big num">{{ total() }}</div>
            <div class="what">
              <strong>Last start-up</strong>
              <span>{{ started() }}</span>
            </div>
            @if (b.verdict) { <p class="verdict">{{ b.verdict }}</p> }
          </section>

          @if (hasChart()) {
            <section class="card chartcard" aria-label="Recent boots">
              <h2 class="sec">Time until the system was up <span class="faint muted">seconds, last {{ b.boots.length }} boots</span></h2>
              <dy-chart [options]="option()" label="Seconds until the system was up for each recent boot, starting at 0" />
            </section>
          }

          <h2 class="sec">What slowed start-up</h2>
          @if (b.slowest.length > 0) {
            <pd-table [toolbar]="false" label="What slowed start-up" [rows]="b.slowest" [columns]="slowColumns" [rowKey]="slowKey" [filterable]="false" height="window" />
          } @else if (b.stepsUnavailable; as why) {
            <p class="note" role="status"><dy-icon name="info" /><span>{{ why }}</span></p>
          } @else {
            <div class="none"><dy-icon name="check" /><strong>Nothing slowed the last start-up</strong><span>No service or program was reported as slow.</span></div>
          }
          @if (b.slowest.length > 0 && b.stepsUnavailable; as why) {
            <p class="note" role="status"><dy-icon name="info" /><span>{{ why }}</span></p>
          }
        }
      }
    </dy-async>
  `,
  styles: [
    EXTRAS_STYLES,
    `
      .lead { display: flex; align-items: center; gap: var(--sp-5); flex-wrap: wrap; }
      .big { font-size: 44px; font-weight: var(--fw-semibold); line-height: 1; letter-spacing: -0.02em; }
      .what { display: grid; gap: 2px; font-size: var(--fs-sm); color: var(--text-3); }
      .what strong { font-size: var(--fs-md); font-weight: var(--fw-semibold); color: var(--text-1); }
      .verdict { margin: 0; flex: 1 1 320px; color: var(--text-2); }
      .chartcard { padding: var(--sp-3) var(--sp-4); --chart-h: 200px; }
      .chartcard .sec { margin: 0 0 var(--sp-2); }
    `,
  ],
})
export class ExtrasBoot implements OnInit {
  protected readonly store = inject(ExtrasStore);
  private readonly theme = inject(ThemeService);
  protected readonly data = computed(() => this.store.boot().data);
  protected readonly secs = formatSeconds;
  protected readonly slowKey = (s: BootStep): string => s.name;
  protected readonly slowColumns: PdColumn<BootStep>[] = [
    { key: 'name', header: 'Service or program', value: (s) => s.name, required: true, sortable: false, width: 'minmax(0, 1fr)' },
    { key: 'secs', header: 'Delay', value: (s) => s.secs, text: (s) => formatSeconds(s.secs), align: 'end', sortable: false, width: '120px' },
  ];
  protected readonly total = computed(() => {
    const b = this.data();
    return formatSeconds(b?.lastTotalSecs ?? b?.boots[0]?.bootSecs ?? null);
  });
  protected readonly started = computed(() => {
    const at = this.data()?.boots[0]?.at;
    return at === undefined ? 'Start time not known' : `Started ${formatDayTime(at)}`;
  });
  protected readonly hasChart = computed(() => (this.data()?.boots.filter((b) => b.bootSecs !== null).length ?? 0) >= 2);
  protected readonly option = computed(() => bootOption(this.data()?.boots ?? [], this.theme.tokens()));

  ngOnInit(): void {
    void this.store.ensure('boot');
  }
}
