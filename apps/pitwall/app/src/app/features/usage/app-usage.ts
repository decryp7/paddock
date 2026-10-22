import { ChangeDetectionStrategy, Component, computed, effect, inject, input, signal, untracked } from '@angular/core';
import type { EChartsCoreOption } from 'echarts/core';
import type { AppPermissions, LimitStatus, RequestRecord } from '../../core/api.types';
import { ThemeService } from '../../core/theme';
import { usageChart, type UsageMetric } from '../../shared/charts/chart-options';
import { EChart } from '../../shared/charts/echart';
import { bytes, clock, compact, duration, int } from '../../shared/format';
import { LIMIT_LABEL, limitStateLook, statusTone } from '../../shared/labels';
import { EmptyState } from '../../shared/ui/empty-state';
import { Panel } from '../../shared/ui/panel';
import { CacheTiles } from './cache-tiles';
import { barWidth, limitValue, tokensOf, USAGE_RANGES, USAGE_RANGE_TEXT } from './usage-format';
import { UsageStore } from './usage.store';

const METRICS: { id: UsageMetric; label: string }[] = [
  { id: 'tokens', label: 'Tokens' },
  { id: 'requests', label: 'Requests' },
  { id: 'memory', label: 'Peak memory' },
];

/** One app's usage over time, where each of its limits stands, and its latest requests (metadata only). */
@Component({
  selector: 'pw-app-usage',
  imports: [Panel, EChart, EmptyState, CacheTiles],
  providers: [UsageStore],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <pw-panel heading="Usage over time" icon="activity" [sub]="rangeText()" [loading]="store.loading()" [error]="store.error()" [ready]="!!store.series()" [skeletonLines]="6" (retry)="store.reload()">
      <div panel-actions class="controls">
        <div class="seg" role="radiogroup" aria-label="Metric">
          @for (m of metrics; track m.id) {
            <button type="button" role="radio" [attr.aria-checked]="metric() === m.id" (click)="metric.set(m.id)">{{ m.label }}</button>
          }
        </div>
        <div class="seg" role="radiogroup" aria-label="Usage range">
          @for (r of ranges; track r) {
            <button type="button" role="radio" [attr.aria-checked]="store.range() === r" (click)="store.select(r)">{{ r }}</button>
          }
        </div>
      </div>
      @if (store.series(); as s) {
        <dl class="figures" aria-label="Totals for the range">
          <div><dt>Requests</dt><dd class="num">{{ intText(s.totals.requests) }}</dd></div>
          <div><dt>Prompt tokens</dt><dd class="num">{{ count(s.totals.promptTokens) }}</dd></div>
          <div><dt>Completion tokens</dt><dd class="num">{{ count(s.totals.completionTokens) }}</dd></div>
          <div><dt>Errors</dt><dd class="num" [class.bad]="s.totals.errors > 0">{{ intText(s.totals.errors) }}</dd></div>
          <div><dt>Peak memory</dt><dd class="num">{{ s.totals.peakMemoryBytes ? size(s.totals.peakMemoryBytes) : '—' }}</dd></div>
        </dl>
        <pw-cache-tiles class="cache" [totals]="s.totals" />
        <pw-echart [option]="option()" [themeKey]="chartKey()" height="240px" [label]="chartLabel()" />
        <p class="note faint">Peak memory is the footprint of the largest model that served this app in each period. The server keeps hourly figures for 7 days and daily figures for 90 days, on this machine only.</p>
      }
    </pw-panel>

    <pw-panel heading="Limits" icon="shield" sub="set under Permissions" [loading]="store.loading() && !store.detail()" [ready]="!!store.detail()">
      @if (limits().length === 0) {
        <p class="none">No limits set: this app can use Pitwall without a request or token budget. Add limits under Permissions to cap it.</p>
      } @else {
        <ul class="limits" aria-label="Limits and how much of each is used">
          @for (l of limits(); track l.kind) {
            <li [attr.data-state]="l.state">
              <div class="head">
                <span class="name">{{ label[l.kind].label }}</span>
                @if (l.state !== 'ok') {
                  <span class="status" [attr.data-tone]="look(l).tone">{{ look(l).label }}</span>
                }
                <span class="faint reset">{{ label[l.kind].resets }}</span>
              </div>
              <div class="bar" role="progressbar" [attr.aria-label]="label[l.kind].label" aria-valuemin="0" aria-valuemax="100" [attr.aria-valuenow]="width(l.percent)" [attr.aria-valuetext]="l.percent + '% used'">
                <span class="fill" [style.width.%]="width(l.percent)"></span>
              </div>
              <div class="nums num">
                <span>{{ value(l, l.used) }} of {{ value(l, l.limit) }}</span>
                <b>{{ l.percent }}%</b>
              </div>
              @if (l.message) {
                <p class="msg">{{ l.message }}</p>
              }
            </li>
          }
        </ul>
      }
    </pw-panel>

    <pw-panel heading="Recent requests" icon="requests" [sub]="recent().length ? recent().length + ' latest' : ''" [flush]="true" [loading]="store.loading() && !store.detail()" [ready]="!!store.detail()">
      @if (recent().length === 0) {
        <pw-empty-state icon="requests" heading="No requests yet" text="Requests from this app appear here with their model, status and size. Prompt text is never shown here." />
      } @else {
        <table class="table recent">
          <thead>
            <tr>
              <th scope="col">Time</th>
              <th scope="col">Endpoint</th>
              <th scope="col">Model</th>
              @if (hasFailures()) {
                <th scope="col">Status</th>
              }
              <th scope="col" class="right">Tokens</th>
              <th scope="col" class="right">Took</th>
            </tr>
          </thead>
          <tbody>
            @for (r of recent(); track r.id) {
              <tr>
                <td class="muted num">{{ time(r.at) }}</td>
                <td class="mono ep">{{ r.endpoint }}</td>
                <td>{{ r.modelId ?? '—' }}</td>
                @if (hasFailures()) {
                  <td>
                    @if (r.status >= 400) {
                      <span class="status" [attr.data-tone]="tone(r.status)">{{ r.status }} failed</span>
                    }
                  </td>
                }
                <td class="right num">{{ count(tokens(r)) }}</td>
                <td class="right num">{{ dur(r.totalMs) }}</td>
              </tr>
            }
          </tbody>
        </table>
      }
    </pw-panel>
  `,
  styles: `
    :host {
      display: flex;
      flex-direction: column;
      gap: var(--sp-3);
    }
    .cache {
      margin-bottom: var(--sp-3);
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
        height: 24px;
        padding: 0 var(--sp-3);
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
    /* The totals are a row of figures on the card, not a boxed grid inside it. */
    .figures {
      display: grid;
      grid-template-columns: repeat(5, minmax(0, 1fr));
      gap: var(--sp-4);
      margin: 0 0 var(--sp-3);
      padding-bottom: var(--sp-3);
      border-bottom: 1px solid var(--border-soft);
      dt {
        font-size: var(--fs-sm);
        color: var(--text-2);
      }
      dd {
        margin: 2px 0 0;
        font-size: var(--fs-lg);
        font-weight: var(--fw-semibold);
      }
      .bad {
        color: var(--bad);
      }
    }
    .note {
      margin: var(--sp-2) 0 0;
      font-size: var(--fs-sm);
    }
    .none {
      margin: 0;
      font-size: var(--fs-sm);
      color: var(--text-2);
    }
    .limits {
      display: flex;
      flex-direction: column;
      gap: var(--sp-4);
      margin: 0;
      padding: 0;
      list-style: none;
    }
    .head {
      display: flex;
      align-items: center;
      gap: var(--sp-2);
      .name {
        font-weight: var(--fw-medium);
      }
      .reset {
        margin-left: auto;
        font-size: var(--fs-sm);
      }
    }
    .bar {
      height: 8px;
      margin: var(--sp-2) 0 var(--sp-1);
      overflow: hidden;
      background: var(--bg-sunken);
      border: 1px solid var(--border-soft);
      border-radius: var(--r-full);
      .fill {
        display: block;
        height: 100%;
        background: var(--text-3);
        border-radius: var(--r-full);
      }
    }
    li[data-state='warning'] .fill {
      background: var(--warn);
    }
    li[data-state='exceeded'] .fill {
      background: var(--bad);
    }
    .nums {
      display: flex;
      justify-content: space-between;
      font-size: var(--fs-sm);
      color: var(--text-2);
    }
    .msg {
      margin: var(--sp-1) 0 0;
      font-size: var(--fs-sm);
      color: var(--text-2);
    }
    .recent {
      th:first-child,
      td:first-child {
        padding-left: var(--sp-4);
      }
      th:last-child,
      td:last-child {
        padding-right: var(--sp-4);
      }
    }
    .ep {
      max-width: 220px;
      font-size: var(--fs-sm);
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
    }
  `,
})
export class AppUsage {
  readonly appId = input.required<string>();
  readonly appName = input.required<string>();
  /** The app's permissions: when they change (limits edited and saved) the limit statuses are fetched again. */
  readonly permissions = input<AppPermissions | null>(null);
  protected readonly store = inject(UsageStore);
  private readonly theme = inject(ThemeService);

  protected readonly metrics = METRICS;
  protected readonly ranges = USAGE_RANGES;
  protected readonly metric = signal<UsageMetric>('tokens');
  protected readonly label = LIMIT_LABEL;
  protected readonly look = (l: LimitStatus) => limitStateLook(l.state);
  protected readonly value = (l: LimitStatus, n: number) => limitValue(l.kind, n);
  protected readonly width = barWidth;
  protected readonly tone = statusTone;
  protected readonly time = (at: number) => clock(at);
  protected readonly count = compact;
  protected readonly intText = int;
  protected readonly size = bytes;
  protected readonly dur = duration;
  protected readonly tokens = (r: RequestRecord) => r.promptTokens + r.completionTokens;

  protected readonly rangeText = computed(() => USAGE_RANGE_TEXT[this.store.range()]);
  protected readonly limits = computed(() => this.store.detail()?.limits ?? []);
  protected readonly recent = computed(() => this.store.detail()?.recent ?? []);
  /** The Status column exists only when something failed: a column of nothing is noise. */
  protected readonly hasFailures = computed(() => this.recent().some((r) => r.status >= 400));
  protected readonly chartKey = computed(() => `${this.theme.resolved()}:${this.store.range()}:${this.metric()}`);
  protected readonly option = computed<EChartsCoreOption>(() => {
    const s = this.store.series();
    return s ? usageChart(s, this.metric(), 'kind', this.theme.tokens()) : {};
  });
  protected readonly chartLabel = computed(() => {
    const s = this.store.series();
    const what = this.metric() === 'tokens' ? 'Prompt and completion tokens' : this.metric() === 'requests' ? 'Successful and failed requests' : 'Peak memory';
    return `${what} of ${this.appName()} over ${this.rangeText()}${s ? `, ${compact(tokensOf(s.totals))} tokens in total` : ''}`;
  });

  constructor() {
    effect(() => {
      const id = this.appId();
      this.permissions();
      untracked(() => void this.store.show(id));
    });
  }
}
