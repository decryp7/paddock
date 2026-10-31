import { FullTextTip } from '../../shared/full-text-tip';
import type { Measured } from '../../core/api.types';
import { showMeasured } from '../../core/measured';
import { ChangeDetectionStrategy, Component, computed, inject, input } from '@angular/core';
import { Icon } from '../../shared/icon';
import type { IconName } from '../../shared/icons';
import { formatBytes, formatPercent } from '../../shared/format';
import { GRADE_LABEL, GRADE_TONE } from '../../shared/labels';
import { Ring } from '../../shared/ring';
import { Sparkline } from '../../shared/sparkline';
import { DiagnosisStore } from '../../state/diagnosis.store';
import { HealthStore } from '../../state/health.store';
import { LiveStore } from '../../state/live.store';
import { ANOMALY_ICON, ANOMALY_LABEL } from '../../shared/labels';
import type { DashboardKind, DashboardWidget, MetricReading, Placement } from '../../core/generated/widgets';
import { WidgetLayouts } from '../../state/widget-layouts';
import { AnimatedNumber } from './animated-number';
import { HealthHero } from '../dashboard/health-hero';
import { CpuTile } from '../dashboard/tiles';
import { DiagnosisCard } from '../dashboard/diagnosis-card';
import { EventsFeed } from '../dashboard/events-feed';
import { ConsumersPanel, DiskTrendPanel, HistoryPanel } from '../dashboard/history-panels';
import { CountersStore } from '../../state/counters.store';
import { counterReading } from './counter-reading';
import { sentenceCase, seriesFor } from './series';
import { WidgetChoicesStore } from '../../state/widget-choices';

/** Kinds that draw a panel of their own and ignore the metric. */
const PANEL_KINDS: readonly DashboardKind[] = ['health', 'issues', 'cpu', 'diagnosis', 'events', 'history', 'consumers', 'diskTrend'];
import { MatTooltip } from '@angular/material/tooltip';

const ICON: Record<DashboardKind, IconName> = {
  graph: 'trend',
  bigNumber: 'gauge',
  bars: 'reports',
  ring: 'gauge',
  heatmap: 'cpu',
  topProcesses: 'processes',
  issues: 'alert',
  health: 'health',
  cpu: 'cpu',
  diagnosis: 'why',
  events: 'bell',
  history: 'trend',
  consumers: 'processes',
  diskTrend: 'disk',
};

/**
 * One dashboard widget, drawn from its configuration: a graph, big number, bars, ring, per-core heatmap, top processes,
 * issues or the health score. Neutral by default; the hero uses the accent, and a threshold colour only appears when a
 * threshold is crossed (always with the number printed). Values glide to their new reading.
 */
@Component({
  selector: 'dy-widget',
  imports: [FullTextTip, MatTooltip, Icon, Ring, Sparkline, AnimatedNumber, HealthHero, CpuTile, DiagnosisCard, EventsFeed, HistoryPanel, ConsumersPanel, DiskTrendPanel],
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: { '[attr.role]': 'ownCard() ? null : "group"', '[attr.aria-label]': 'ownCard() ? null : title()', '[attr.data-kind]': 'widget().kind', '[class.own-card]': 'ownCard()' },
  template: `
    @if (!ownCard()) {
    <header>
      <dy-icon [name]="icon()" />
      <h3 class="truncate">{{ title() }}</h3>
      @if (reading().reason && widget().kind !== 'topProcesses' && widget().kind !== 'health' && widget().kind !== 'issues') {
        <span class="faint">Not available</span>
      }
    </header>
    }
    @switch (widget().kind) {
      @case ('bigNumber') {
        <div class="hero" [attr.data-tone]="reading().tone"><b><dy-animated-number [value]="reading().text" /></b><small>{{ reading().unit }}</small></div>
        @if (reading().reason; as why) {
          <p class="note">{{ why }}</p>
        }
      }
      @case ('graph') {
        <div class="hero" [attr.data-tone]="reading().tone"><b><dy-animated-number [value]="reading().text" /></b><small>{{ reading().unit }}</small></div>
        @if (reading().reason; as why) {
          <p class="note">{{ why }}</p>
        } @else if (series(); as s) {
          <dy-sparkline [values]="s.values" [values2]="s.values2" [max]="percent() ? 100 : null" [zoom]="zoomed() && percent()" [fill]="widget().options.fill" [color]="accent()" color2="var(--text-3)" [label]="title() + ' over the last minute'" style="--spark-h: 48px" />
        } @else {
          <p class="note">No live history is kept for this metric.</p>
        }
      }
      @case ('bars') {
        <div class="hero" [attr.data-tone]="reading().tone"><b><dy-animated-number [value]="reading().text" /></b><small>{{ reading().unit }}</small></div>
        <div class="bar-track" [attr.data-tone]="barTone()" role="img" [attr.aria-label]="title() + ' ' + reading().text + ' ' + reading().unit"><span [style.width.%]="reading().percent ?? 0"></span></div>
      }
      @case ('ring') {
        <div class="ringrow">
          <dy-ring [value]="reading().percent ?? 0" [tone]="ringTone()" [label]="title() + ' ' + reading().text + reading().unit" style="--ring-size: 84px">
            <span class="rn"><dy-animated-number [value]="reading().text" /></span>
          </dy-ring>
          <p class="note">{{ reading().reason ?? title() }}</p>
        </div>
      }
      @case ('heatmap') {
        <div class="cells" role="img" [attr.aria-label]="'Per-core load: ' + cores().map(pct).join(', ')">
          @for (v of cores(); track $index) {
            <span class="cell" [style.background]="cellColor(v)" [matTooltip]="'Core ' + round($index + 1) + ': ' + pct(v)"></span>
          }
        </div>
        <p class="note num">{{ round(cores().length) }} cores · busiest {{ pct(busiest()) }}</p>
      }
      @case ('topProcesses') {
        <ol>
          @for (g of top(); track g.id) {
            <li><span class="truncate">{{ g.name }}</span><span class="num">{{ topValue(g) }}</span></li>
          } @empty {
            <li class="faint">Waiting for the first reading…</li>
          }
        </ol>
      }
      @case ('issues') {
        @if (anomalies().length === 0) {
          <p class="note ok"><dy-icon name="check" /> Nothing unusual found.</p>
        }
        <ul class="issues">
          @for (a of anomalies(); track a.id) {
            <li>
              <span class="ic" aria-hidden="true"><dy-icon [name]="anomalyIcon(a.kind)" /></span>
              <span class="tx"><strong class="truncate" [dyFullText]="a.title">{{ a.title }}</strong><small class="truncate" [dyFullText]="label(a.kind) + ' · ' + a.app">{{ label(a.kind) }} · {{ a.app }}</small></span>
            </li>
          }
        </ul>
      }
      @case ('cpu') { <dy-cpu-tile /> }
      @case ('diagnosis') { <dy-diagnosis-card /> }
      @case ('events') { <dy-events-feed /> }
      @case ('history') { <dy-history-panel /> }
      @case ('consumers') { <dy-consumers-panel /> }
      @case ('diskTrend') { <dy-disk-trend-panel /> }
      @case ('health') {
        @if (placement().span > 3) {
          <dy-health-hero />
        } @else {
        @if (health.report(); as r) {
          <div class="ringrow">
            <dy-ring [value]="r.score" [tone]="gradeTone()" [label]="'Health score ' + r.score" style="--ring-size: 84px"><span class="rn num">{{ round(r.score) }}</span></dy-ring>
            <div><strong class="grade" [attr.data-tone]="gradeTone()">{{ grade() }}</strong><p class="note">Health score</p></div>
          </div>
        } @else {
          <p class="note">{{ health.reportError()?.message ?? 'Reading the health score…' }}</p>
        }
        }
      }
    }
  `,
  styles: `
    :host { display: flex; flex-direction: column; gap: var(--sp-3); min-width: 0; min-height: 132px; padding: var(--sp-4); background: var(--surface-2); border: 1px solid var(--border); border-radius: var(--r-lg); box-shadow: var(--shadow-1); }
    :host(.own-card) > * { display: block; height: 100%; min-width: 0; }
    :host(.own-card) { display: block; }
    :host(.own-card) { padding: 0; background: none; border: 0; border-radius: 0; box-shadow: none; min-height: 0; }
    header { display: flex; flex-wrap: wrap; align-items: center; gap: var(--sp-2); color: var(--text-2); }
    header dy-icon { --icon-size: 16px; color: var(--text-3); }
    h3 { font-size: var(--fs-sm); font-weight: var(--fw-semibold); color: var(--text-2); flex: 1; }
    .faint { color: var(--text-3); font-size: var(--fs-xs); }
    /* "Not available" sits under the title instead of squeezing it to an ellipsis in a narrow card. */
    header .faint { flex: 0 0 100%; }
    .hero[data-tone='warn'] b { color: var(--warn); }
    .hero[data-tone='bad'] b { color: var(--bad); }
    .note { color: var(--text-3); font-size: var(--fs-xs); display: flex; align-items: center; gap: 6px; }
    .note.ok { color: var(--ok); }
    .bar-track { height: 6px; }
    .ringrow { display: flex; align-items: center; gap: var(--sp-4); }
    .rn { font: var(--fw-semibold) var(--fs-xl) / 1 var(--font-display); font-variant-numeric: tabular-nums; }
    .grade { font: var(--fw-semibold) var(--fs-lg) / 1.2 var(--font-display); }
    .grade[data-tone='ok'] { color: var(--ok); } .grade[data-tone='warn'] { color: var(--warn); } .grade[data-tone='bad'] { color: var(--bad); }
    .cells { display: grid; grid-auto-flow: column; grid-auto-columns: 1fr; gap: 2px; height: 36px; }
    .cell { border-radius: 3px; background: var(--surface-active); }
    ol, ul { list-style: none; margin: 0; padding: 0; display: grid; gap: 2px; font-size: var(--fs-sm); }
    ol li { display: flex; justify-content: space-between; gap: var(--sp-2); padding: 4px 0; border-bottom: 1px solid var(--border-subtle); }
    ul li { display: flex; gap: var(--sp-2); min-width: 0; }
    .issues { gap: var(--sp-2); }
    .issues li { align-items: center; gap: var(--sp-3); }
    .issues .ic { display: grid; place-items: center; width: 26px; height: 26px; border-radius: var(--r-sm); background: var(--warn-soft); color: var(--warn); flex: none; }
    .issues .tx { display: grid; min-width: 0; line-height: 1.3; }
    .issues strong { font-size: var(--fs-base); font-weight: var(--fw-semibold); }
    .issues small { color: var(--text-3); font-size: var(--fs-xs); }
  `,
})
export class WidgetView {
  /** The dashboard placement to draw (its span is the width on the 12-column board). */
  readonly placement = input.required<Placement<DashboardWidget>>();
  /** The engine's reading of this widget's metric (null until the first answer). */
  readonly metricReading = input<MetricReading | null>(null);
  protected readonly widget = computed(() => this.placement().widget);
  private readonly layouts = inject(WidgetLayouts);
  private readonly choices = inject(WidgetChoicesStore);
  protected readonly live = inject(LiveStore);
  protected readonly health = inject(HealthStore);
  private readonly diagnosis = inject(DiagnosisStore);
  private readonly counters = inject(CountersStore);

  protected readonly title = computed(() => (this.widget().kind === 'topProcesses' ? `Top apps by ${sentenceCase(this.choices.title(this.widget().metric))}` : PANEL_KINDS.includes(this.widget().kind) ? this.kindTitle() : this.choices.title(this.widget().metric)));
  /** The panel kinds are cards of their own (they bring header, loading and error states). */
  protected readonly ownCard = computed(() => ['cpu', 'diagnosis', 'events', 'history', 'consumers', 'diskTrend'].includes(this.widget().kind) || (this.widget().kind === 'health' && this.placement().span > 3));
  protected readonly icon = computed(() => ICON[this.widget().kind]);
  /** The performance counter this widget shows, when it shows one. */
  private readonly counterPath = computed(() => {
    const m = this.widget().metric;
    return typeof m === 'object' && 'perfCounter' in m ? m.perfCounter : null;
  });
  protected readonly reading = computed<MetricReading>(() => {
    const path = this.counterPath();
    if (path !== null) return counterReading(this.counters.readings()[path]);
    return this.metricReading() ?? { value: null, text: '—', unit: '', percent: null, tone: 'ok', reason: 'Waiting for the first reading.' };
  });
  /** The engine's tone of the value against the widget's thresholds: only a crossed threshold colours it. */
  protected readonly barTone = computed(() => (this.reading().tone === 'ok' ? null : this.reading().tone));
  protected readonly ringTone = computed<'ok' | 'warn' | 'bad' | 'accent'>(() => (this.reading().tone === 'ok' ? 'accent' : this.reading().tone));
  protected readonly percent = computed(() => this.reading().percent !== null);
  /** The kind's title from the engine's catalogue. */
  private readonly kindTitle = computed(() => this.layouts.dashboard()?.catalogue.kinds.find((k) => k.kind === this.widget().kind)?.title ?? this.widget().kind);
  protected readonly accent = computed(() => this.widget().options.accent ?? 'var(--accent)');
  protected readonly series = computed(() => {
    const path = this.counterPath();
    if (path === null) return seriesFor(this.widget().metric, this.live.series());
    const values = this.counters.trail()[path] ?? [];
    return values.length > 1 ? { values, values2: null } : null;
  });
  protected readonly cores = computed(() => this.live.system()?.cpu.perCore ?? []);
  protected readonly busiest = computed(() => Math.max(0, ...this.cores()));
  protected readonly top = computed(() => {
    const u = this.live.update();
    return ((this.widget().metric === 'memory' ? u?.topMemory : u?.topCpu) ?? []).slice(0, 5);
  });
  protected readonly anomalies = computed(() => this.diagnosis.anomalies().slice(0, 3));
  protected readonly grade = computed(() => (this.health.report() ? GRADE_LABEL[this.health.report()!.grade] : ''));
  protected readonly gradeTone = computed(() => (this.health.report() ? GRADE_TONE[this.health.report()!.grade] : 'accent'));
  protected readonly pct = formatPercent;
  protected readonly round = (v: number) => Math.round(v).toString();
  protected readonly label = (k: keyof typeof ANOMALY_LABEL) => ANOMALY_LABEL[k];
  protected readonly anomalyIcon = (k: keyof typeof ANOMALY_ICON) => ANOMALY_ICON[k];
  /** Slow-moving levels are drawn zoomed so a steady 50% still shows its line. */
  protected readonly zoomed = computed(() => ['memory', 'swap', 'gpuLoad', 'diskFree', 'battery'].includes(String(this.widget().metric)));
  protected readonly topValue = (g: { cpu: Measured<number>; memory: Measured<number> }) => (this.widget().metric === 'memory' ? showMeasured(g.memory, formatBytes) : showMeasured(g.cpu, formatPercent));
  protected readonly cellColor = (v: number) => `color-mix(in srgb, var(--accent) ${Math.round(Math.min(100, Math.max(0, v)) * 0.85)}%, var(--surface-active))`;
}
