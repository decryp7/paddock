import { ChangeDetectionStrategy, Component, computed, effect, inject, input, signal, untracked } from '@angular/core';
import { MatTableModule } from '@angular/material/table';
import { MatTooltipModule } from '@angular/material/tooltip';
import type { EChartsCoreOption } from 'echarts/core';
import type { AppPermissions, LimitStatus, RequestRecord } from '../../core/api.types';
import { ThemeService } from '../../core/theme';
import { usageChart, type UsageMetric } from '../../shared/charts/chart-options';
import { EChart } from '../../shared/charts/echart';
import { bytes, clock, compact, durationIn, durationUnit, int, varies } from '../../shared/format';
import { LIMIT_LABEL, limitStateLook, statusTone } from '../../shared/labels';
import { MatIcon } from '@angular/material/icon';
import { SuEmptyState, SuSegmentedControl, type SuSegment } from '@decryp7/paddock-ui';
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
  imports: [Panel, EChart, SuEmptyState, SuSegmentedControl, MatIcon, CacheTiles, MatTableModule, MatTooltipModule],
  providers: [UsageStore],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <pw-panel heading="Usage over time" icon="activity" [sub]="rangeText()" [loading]="store.loading()" [error]="store.error()" [ready]="!!store.series()" [skeletonLines]="6" (retry)="store.reload()">
      <div panel-actions class="controls">
        <pd-segmented-control label="Metric" [options]="metricSegs" [value]="metric()" (valueChange)="pickMetric($event)" />
        <pd-segmented-control label="Usage range" [options]="rangeSegs" [value]="store.range()" (valueChange)="store.select($any($event))" />
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

    <pw-panel heading="Recent requests" icon="requests" [sub]="recentSub()" [flush]="true" [loading]="store.loading() && !store.detail()" [ready]="!!store.detail()">
      @if (recent().length === 0) {
        <pd-empty-state heading="No requests yet" description="Requests from this app appear here with their model, status and size. Prompt text is never shown here."><mat-icon slot="icon" svgIcon="pw:requests" aria-hidden="true" /></pd-empty-state>
      } @else {
        <table mat-table class="recent" [dataSource]="recent()" [trackBy]="byId" [fixedLayout]="true" [attr.aria-label]="'Recent requests of ' + appName()">
          <ng-container matColumnDef="time">
            <th mat-header-cell *matHeaderCellDef scope="col">Time</th>
            <td mat-cell *matCellDef="let r" class="muted num">{{ time(r.at) }}</td>
          </ng-container>
          <ng-container matColumnDef="endpoint">
            <th mat-header-cell *matHeaderCellDef scope="col">Endpoint</th>
            <td mat-cell *matCellDef="let r" class="mono ep" [matTooltip]="r.endpoint" matTooltipShowDelay="600">{{ r.endpoint }}</td>
          </ng-container>
          <ng-container matColumnDef="model">
            <th mat-header-cell *matHeaderCellDef scope="col">Model</th>
            <td mat-cell *matCellDef="let r" [matTooltip]="r.modelId ?? ''" matTooltipShowDelay="600">{{ r.modelId ?? '—' }}</td>
          </ng-container>
          <ng-container matColumnDef="status">
            <th mat-header-cell *matHeaderCellDef scope="col">Status</th>
            <td mat-cell *matCellDef="let r">
              @if (r.status >= 400) {
                <span class="status" [attr.data-tone]="tone(r.status)">{{ r.status }} failed</span>
              }
            </td>
          </ng-container>
          <ng-container matColumnDef="tokens">
            <th mat-header-cell *matHeaderCellDef scope="col" class="right">Tokens</th>
            <td mat-cell *matCellDef="let r" class="right num">{{ count(tokens(r)) }}</td>
          </ng-container>
          <ng-container matColumnDef="took">
            <th mat-header-cell *matHeaderCellDef scope="col" class="right">Took {{ tookUnit() }}</th>
            <td mat-cell *matCellDef="let r" class="right num">{{ durIn(r.totalMs, tookUnit()) }}</td>
          </ng-container>
          <tr mat-header-row *matHeaderRowDef="columns(); sticky: true"></tr>
          <tr mat-row *matRowDef="let r; columns: columns()"></tr>
        </table>
      }
    </pw-panel>
  `,
  styles: `
    :host {
      display: flex;
      flex-direction: column;
      gap: var(--pd-space-3);
    }
    .recent {
      .mat-column-time {
        width: 72px;
      }
      .mat-column-status {
        width: 96px;
      }
      /* "2,213" and "2.2M" whole: numbers are never cut (design guide 3). */
      .mat-column-tokens,
      .mat-column-took {
        width: 88px;
      }
    }
    .cache {
      margin-bottom: var(--pd-space-3);
    }
    .controls {
      display: flex;
      flex-wrap: wrap;
      justify-content: flex-end;
      gap: var(--pd-space-2);
    }
    /* The totals are a row of figures on the card, not a boxed grid inside it. */
    .figures {
      display: grid;
      grid-template-columns: repeat(5, minmax(0, 1fr));
      gap: var(--pd-space-4);
      margin: 0 0 var(--pd-space-3);
      padding-bottom: var(--pd-space-3);
      border-bottom: 1px solid var(--pd-hairline);
      dt {
        font-size: var(--pd-text-label);
        color: var(--pd-text-muted);
      }
      dd {
        margin: 2px 0 0;
        font-size: var(--pd-text-subtitle);
        font-weight: var(--pd-weight-semibold);
      }
      .bad {
        color: var(--pd-danger);
      }
    }
    .note {
      margin: var(--pd-space-2) 0 0;
      font-size: var(--pd-text-label);
    }
    .none {
      margin: 0;
      font-size: var(--pd-text-label);
      color: var(--pd-text-muted);
    }
    .limits {
      display: flex;
      flex-direction: column;
      gap: var(--pd-space-4);
      margin: 0;
      padding: 0;
      list-style: none;
    }
    .head {
      display: flex;
      align-items: center;
      gap: var(--pd-space-2);
      .name {
        font-weight: var(--pd-weight-medium);
      }
      .reset {
        margin-left: auto;
        font-size: var(--pd-text-label);
      }
    }
    .bar {
      height: 8px;
      margin: var(--pd-space-2) 0 var(--pd-space-1);
      overflow: hidden;
      background: var(--pd-surface-2);
      border: 1px solid var(--pd-hairline);
      border-radius: var(--pd-radius-pill);
      .fill {
        display: block;
        height: 100%;
        background: var(--pd-text-subtle);
        border-radius: var(--pd-radius-pill);
      }
    }
    li[data-state='warning'] .fill {
      background: var(--pd-warning);
    }
    li[data-state='exceeded'] .fill {
      background: var(--pd-danger);
    }
    .nums {
      display: flex;
      justify-content: space-between;
      font-size: var(--pd-text-label);
      color: var(--pd-text-muted);
    }
    .msg {
      margin: var(--pd-space-1) 0 0;
      font-size: var(--pd-text-label);
      color: var(--pd-text-muted);
    }
    .recent {
      .mat-mdc-header-cell:last-child,
      .mat-mdc-cell:last-child {
        padding-right: var(--pd-space-4);
      }
    }
    .ep {
      font-size: var(--pd-text-label);
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

  protected readonly metricSegs: SuSegment[] = METRICS.map((m) => ({ value: m.id, label: m.label }));
  protected readonly rangeSegs: SuSegment[] = USAGE_RANGES.map((r) => ({ value: r, label: r }));
  protected readonly metric = signal<UsageMetric>('tokens');

  /** The segmented control speaks strings: only a known metric is taken. */
  protected pickMetric(v: string): void {
    const m = METRICS.find((x) => x.id === v);
    if (m) this.metric.set(m.id);
  }
  protected readonly label = LIMIT_LABEL;
  protected readonly look = (l: LimitStatus) => limitStateLook(l.state);
  protected readonly value = (l: LimitStatus, n: number) => limitValue(l.kind, n);
  protected readonly width = barWidth;
  protected readonly tone = statusTone;
  protected readonly time = (at: number) => clock(at);
  protected readonly count = compact;
  protected readonly intText = int;
  protected readonly size = bytes;
  protected readonly tokens = (r: RequestRecord) => r.promptTokens + r.completionTokens;

  protected readonly rangeText = computed(() => USAGE_RANGE_TEXT[this.store.range()]);
  protected readonly limits = computed(() => this.store.detail()?.limits ?? []);
  protected readonly recent = computed(() => this.store.detail()?.recent ?? []);
  /** The Status column exists only when something failed: a column of nothing is noise. */
  protected readonly hasFailures = computed(() => this.recent().some((r) => r.status >= 400));
  /** Endpoint and model only when the requests differ in them (UI review 2 rule 7); a shared one is said in the panel's subtitle. */
  protected readonly columns = computed(() => {
    const r = this.recent();
    return [
      'time',
      ...(varies(r, (x) => x.endpoint) ? ['endpoint'] : []),
      ...(varies(r, (x) => x.modelId) ? ['model'] : []),
      ...(this.hasFailures() ? ['status'] : []),
      'tokens',
      'took',
    ];
  });
  protected readonly recentSub = computed(() => {
    const r = this.recent();
    if (!r.length) return '';
    const cols = this.columns();
    const shared = [cols.includes('endpoint') || r.length < 2 ? '' : r[0]!.endpoint, cols.includes('model') || r.length < 2 ? '' : (r[0]!.modelId ?? '')].filter(Boolean);
    return [`${r.length} latest`, ...shared.map((v) => `all ${v}`)].join(' · ');
  });
  protected readonly tookUnit = computed(() => durationUnit(this.recent().map((r) => r.totalMs)));
  protected readonly durIn = durationIn;
  protected readonly byId = (_: number, r: RequestRecord) => r.id;
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
