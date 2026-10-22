import { ChangeDetectionStrategy, Component, OnInit, computed, inject, signal } from '@angular/core';
import { RouterLink } from '@angular/router';
import type { EChartsCoreOption } from 'echarts/core';
import { LiveStore } from '../../core/live.store';
import { ThemeService } from '../../core/theme';
import { usageChart, type UsageMetric } from '../../shared/charts/chart-options';
import { EChart } from '../../shared/charts/echart';
import { bytes, compact, int } from '../../shared/format';
import { seriesColor } from '../../shared/theme-tokens';
import { EmptyState } from '../../shared/ui/empty-state';
import { Panel } from '../../shared/ui/panel';
import { tokensOf, USAGE_RANGES, USAGE_RANGE_TEXT } from './usage-format';
import { UsageStore } from './usage.store';

/** The dashboard's compact usage chart: tokens or requests per app over 24 h, 7 d or 30 d, with a legend of totals. */
@Component({
  selector: 'pw-usage-card',
  imports: [Panel, EChart, EmptyState, RouterLink],
  providers: [UsageStore],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <pw-panel heading="Usage by app" icon="apps" [sub]="rangeText()" [loading]="store.loading()" [error]="store.error()" [ready]="!!store.series()" [skeletonLines]="6" (retry)="store.reload()">
      <div panel-actions class="controls">
        <div class="seg" role="radiogroup" aria-label="Usage metric">
          <button type="button" role="radio" [attr.aria-checked]="metric() === 'tokens'" (click)="metric.set('tokens')">Tokens</button>
          <button type="button" role="radio" [attr.aria-checked]="metric() === 'requests'" (click)="metric.set('requests')">Requests</button>
        </div>
        <div class="seg" role="radiogroup" aria-label="Usage range">
          @for (r of ranges; track r) {
            <button type="button" role="radio" [attr.aria-checked]="store.range() === r" (click)="store.select(r)">{{ r }}</button>
          }
        </div>
      </div>
      @if (store.series(); as s) {
        @if (s.totals.requests === 0) {
          <pw-empty-state icon="apps" heading="No usage yet" text="Requests from your apps appear here, per app, as they are made.">
            <a class="btn sm" routerLink="/apps">Apps & keys</a>
          </pw-empty-state>
        } @else {
          <pw-echart [option]="option()" [themeKey]="chartKey()" height="170px" [label]="chartLabel()" />
          <ul class="totals" aria-label="Usage per app">
            @for (a of s.apps; track a.appId; let i = $index) {
              <li>
                <i class="swatch" [style.background]="color(i)" aria-hidden="true"></i>
                <a routerLink="/apps" [queryParams]="a.appId === 'other' ? null : { app: a.appId }" class="nm">{{ name(a.appId) }}</a>
                <span class="num">{{ count(tokens(a.totals)) }} tokens</span>
                <span class="num faint">{{ intText(a.totals.requests) }} requests</span>
                <span class="num faint">{{ a.totals.peakMemoryBytes ? size(a.totals.peakMemoryBytes) + ' peak' : '' }}</span>
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
      gap: var(--sp-2);
    }
    .seg {
      display: inline-flex;
      padding: 2px;
      background: var(--bg-active);
      border-radius: var(--r-md);
      button {
        height: 22px;
        min-width: 40px;
        padding: 0 var(--sp-2);
        font: inherit;
        font-size: var(--fs-sm);
        font-weight: var(--fw-medium);
        color: var(--text-2);
        background: none;
        border: none;
        border-radius: var(--r-sm);
        cursor: default;
        &:hover {
          color: var(--text);
        }
        &[aria-checked='true'] {
          color: var(--text);
          background: var(--bg-card);
          box-shadow: var(--shadow-1);
        }
      }
    }
    .totals {
      display: grid;
      grid-template-columns: repeat(auto-fit, minmax(300px, 1fr));
      gap: var(--sp-1) var(--sp-4);
      margin: var(--sp-2) 0 0;
      padding: 0;
      list-style: none;
      li {
        display: grid;
        grid-template-columns: 10px minmax(0, 1fr) auto auto auto;
        align-items: center;
        gap: var(--sp-2);
        font-size: var(--fs-sm);
      }
      .swatch {
        width: 10px;
        height: 10px;
        border-radius: 3px;
      }
      .nm {
        overflow: hidden;
        font-weight: var(--fw-medium);
        color: var(--text);
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

  protected readonly ranges = USAGE_RANGES;
  protected readonly metric = signal<Exclude<UsageMetric, 'memory'>>('tokens');
  protected readonly count = compact;
  protected readonly intText = int;
  protected readonly size = bytes;
  protected readonly tokens = tokensOf;
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
