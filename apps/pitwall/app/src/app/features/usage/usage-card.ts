import { ChangeDetectionStrategy, Component, OnInit, computed, inject, signal } from '@angular/core';
import { RouterLink } from '@angular/router';
import type { EChartsCoreOption } from 'echarts/core';
import type { UsageCounters } from '../../core/api.types';
import { LiveStore } from '../../core/live.store';
import { ThemeService } from '../../core/theme';
import { usageChart, type UsageMetric } from '../../shared/charts/chart-options';
import { EChart } from '../../shared/charts/echart';
import { bytes, compact, int } from '../../shared/format';
import { seriesColor } from '../../shared/theme-tokens';
import { Panel } from '../../shared/ui/panel';
import { CacheTiles } from './cache-tiles';
import { cacheHitRate, cacheHitRateText, tokensOf, USAGE_RANGES, USAGE_RANGE_TEXT } from './usage-format';
import { UsageStore } from './usage.store';
import { MatButton } from '@angular/material/button';
import { MatIcon } from '@angular/material/icon';
import { SuEmptyState, SuSegmentedControl, type SuSegment } from '@decryp7/paddock-ui';

/** The dashboard's compact usage chart: tokens or requests per app over 24 h, 7 d or 30 d, with a legend of totals. */
@Component({
  selector: 'pw-usage-card',
  imports: [MatButton, MatIcon, SuEmptyState, SuSegmentedControl, Panel, EChart, RouterLink, CacheTiles],
  providers: [UsageStore],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <pw-panel heading="Usage by app" icon="apps" [sub]="rangeText()" [loading]="store.loading()" [error]="store.error()" [ready]="!!store.series()" [skeletonLines]="6" (retry)="store.reload()">
      <div panel-actions class="controls">
        <pd-segmented-control label="Usage metric" [options]="metricSegs" [value]="metric()" (valueChange)="metric.set($event === 'requests' ? 'requests' : 'tokens')" />
        <pd-segmented-control label="Usage range" [options]="rangeSegs" [value]="store.range()" (valueChange)="store.select($any($event))" />
      </div>
      @if (store.series(); as s) {
        @if (s.totals.requests === 0) {
          <pd-empty-state heading="No usage yet" description="Requests from your apps appear here, per app, as they are made.">
            <mat-icon slot="icon" svgIcon="pw:apps" aria-hidden="true" />
            <a slot="action" matButton="outlined" routerLink="/apps">Apps & keys</a>
          </pd-empty-state>
        } @else {
          <pw-echart [option]="option()" [themeKey]="chartKey()" height="170px" [label]="chartLabel()" />
          <pw-cache-tiles class="cache" [totals]="s.totals" />
          <ul class="totals" aria-label="Usage per app">
            @for (a of s.apps; track a.appId; let i = $index) {
              <li>
                <i class="swatch" [style.background]="color(i)" aria-hidden="true"></i>
                <a routerLink="/apps" [queryParams]="a.appId === 'other' ? null : { app: a.appId }" class="nm">{{ name(a.appId) }}</a>
                <span class="num">{{ count(tokens(a.totals)) }} tokens</span>
                <span class="num faint">{{ intText(a.totals.requests) }} requests</span>
                <span class="num faint">{{ a.totals.peakMemoryBytes ? size(a.totals.peakMemoryBytes) + ' peak' : '' }}</span>
                <span class="num faint" [title]="'Cache hits: ' + intText(a.totals.cacheHits)">{{ hasRate(a.totals) ? rateText(a.totals) + ' cached' : '' }}</span>
              </li>
            }
          </ul>
        }
      }
    </pw-panel>
  `,
  styles: `
    :host {
      display: block;
      min-width: 0;
    }
    .controls {
      display: flex;
      gap: var(--pd-space-2);
    }
    .cache {
      margin-top: var(--pd-space-3);
    }
    .totals {
      display: grid;
      grid-template-columns: repeat(auto-fit, minmax(380px, 1fr));
      gap: var(--pd-space-1) var(--pd-space-4);
      margin: var(--pd-space-2) 0 0;
      padding: 0;
      list-style: none;
      li {
        display: grid;
        grid-template-columns: 10px minmax(0, 1fr) auto auto auto auto;
        align-items: center;
        gap: var(--pd-space-2);
        font-size: var(--pd-text-label);
      }
      .swatch {
        width: 10px;
        height: 10px;
        border-radius: 3px;
      }
      .nm {
        overflow: hidden;
        font-weight: var(--pd-weight-medium);
        color: var(--pd-text);
        text-decoration: none;
        text-overflow: ellipsis;
        white-space: nowrap;
        &:hover {
          text-decoration: underline;
        }
      }
    }
  `,
})
export class UsageCard implements OnInit {
  protected readonly store = inject(UsageStore);
  private readonly live = inject(LiveStore);
  private readonly theme = inject(ThemeService);

  protected readonly rangeSegs: SuSegment[] = USAGE_RANGES.map((r) => ({ value: r, label: r }));
  protected readonly metricSegs: SuSegment[] = [
    { value: 'tokens', label: 'Tokens' },
    { value: 'requests', label: 'Requests' },
  ];
  protected readonly metric = signal<Exclude<UsageMetric, 'memory'>>('tokens');
  protected readonly count = compact;
  protected readonly intText = int;
  protected readonly size = bytes;
  protected readonly tokens = tokensOf;
  protected readonly rateText = cacheHitRateText;
  protected hasRate(c: UsageCounters): boolean {
    return cacheHitRate(c) !== null;
  }
  protected readonly rangeText = computed(() => USAGE_RANGE_TEXT[this.store.range()]);
  private readonly names = computed(() => new Map(this.live.apps().map((a) => [a.id, a.name])));
  protected readonly chartKey = computed(() => `${this.theme.resolved()}:${this.store.range()}:${this.metric()}`);
  protected readonly option = computed<EChartsCoreOption>(() => {
    const s = this.store.series();
    return s ? usageChart(s, this.metric(), 'app', this.theme.tokens(), (id) => this.name(id)) : {};
  });
  protected readonly chartLabel = computed(() => {
    const s = this.store.series();
    return `${this.metric() === 'tokens' ? 'Tokens' : 'Requests'} per app over ${this.rangeText()}${s ? `, ${compact(tokensOf(s.totals))} tokens in total` : ''}`;
  });

  /** The app's display name; `other` is the apps beyond the busiest eight. */
  protected name(id: string): string {
    return id === 'other' ? 'Other apps' : (this.names().get(id) ?? id);
  }

  protected color(i: number): string {
    return seriesColor(this.theme.tokens(), i);
  }

  ngOnInit(): void {
    void this.store.show(null, '24h');
  }
}
