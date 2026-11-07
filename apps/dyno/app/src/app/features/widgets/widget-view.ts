import { MatTooltip } from '@angular/material/tooltip';
import { SuSparkline } from '@decryp7/paddock-ui';
import { formatNumber } from '@decryp7/paddock-ui/format';
import { PdSkeleton } from '@decryp7/paddock-ui/widgets';
import { FullTextTip } from '../../shared/full-text-tip';
import type { Measured } from '../../core/api.types';
import { showMeasured } from '../../core/measured';
import { ChangeDetectionStrategy, Component, computed, inject, input } from '@angular/core';
import { Icon } from '../../shared/icon';
import { formatBytes, formatPercent, formatRate } from '../../shared/format';
import { GRADE_LABEL, GRADE_TONE } from '../../shared/labels';
import { coreHeat } from '../../shared/heat';
import { Ring } from '../../shared/ring';
import { DiagnosisStore } from '../../state/diagnosis.store';
import { HealthStore } from '../../state/health.store';
import { LiveStore, SPARK_CAPACITY } from '../../state/live.store';
import { ANOMALY_ICON, ANOMALY_LABEL } from '../../shared/labels';
import type { Cells, DashboardKind, DashboardWidget, MetricReading, Placement } from '../../core/generated/widgets';
import { ReadingSource } from '../../core/reading-source';
import { AnimatedNumber } from './animated-number';
import { HealthHero } from '../dashboard/health-hero';
import { CpuTile } from '../dashboard/tiles';
import { DiagnosisCard } from '../dashboard/diagnosis-card';
import { EventsFeed } from '../dashboard/events-feed';
import { ConsumersPanel, DiskTrendPanel, HistoryPanel } from '../dashboard/history-panels';
import { CountersStore } from '../../state/counters.store';
import { counterReading } from './counter-reading';
import { seriesFor } from './series';
import { WidgetChoicesStore } from '../../state/widget-choices';
import type { DyValue } from './dashboard-states';

/** The first width, in columns, at which the health widget shows its hero (the former "span greater than 3"). */
export const HERO_MIN_WIDTH = 4;
/** From this many rows the health hero lists three reasons, below it two (the third would be cut). */
export const HERO_ROWS_FOR_THREE = 11;

/** The placement the body reads while there is no value (never drawn: the twin is). */
const NONE: Placement<DashboardWidget, Cells> = { id: '', x: 0, y: 0, w: 1, h: 1, widget: { kind: 'bigNumber', metric: 'cpuTotal', options: { rangeSecs: 300, showLabel: true, fill: true, accent: null }, thresholds: { warning: null, critical: null } } };

/**
 * The body of one dashboard widget, drawn from its configuration: a graph, big number, bars, ring, per-core heatmap, top
 * processes, issues, the health score or one of the panels. Content only: the shared grid's frame draws the title, the menu,
 * the focus ring and the loading, empty and error states (family ADR 0044 §5). Without a value the widget is loading and the
 * body is its skeleton twin (`pdSkeleton` blocks of the ready height). Neutral by default; the hero uses the accent, and a
 * threshold colour only appears when a threshold is crossed (always with the number printed). Values glide to their reading.
 */
@Component({
  selector: 'dy-widget',
  imports: [FullTextTip, MatTooltip, ReadingSource, PdSkeleton, Icon, Ring, SuSparkline, AnimatedNumber, HealthHero, CpuTile, DiagnosisCard, EventsFeed, HistoryPanel, ConsumersPanel, DiskTrendPanel],
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: { '[attr.data-kind]': 'kind()', '[style.--wv-rows]': 'rows()' },
  template: `
    @if (value() === null) {
      <div class="twin">
        <b pdSkeleton class="sk-big">00.0</b>
        <span pdSkeleton class="sk-line">Loading the reading</span>
        <span pdSkeleton class="sk-line sk-short">Loading</span>
      </div>
    } @else {
    @switch (kind()) {
      @case ('bigNumber') {
        <div class="hero" [attr.data-tone]="reading().tone"><b><dy-animated-number [value]="reading().text" /></b><small>{{ reading().unit }}</small></div>
        @if (reading().reason; as why) {
          <p class="note"><span>Not available. {{ why }}</span></p>
        }
        @if (thermal()) { <dy-reading-source [via]="thermalVia()" /> }
      }
      @case ('graph') {
        <div class="hero" [attr.data-tone]="reading().tone"><b><dy-animated-number [value]="reading().text" /></b><small>{{ reading().unit }}</small></div>
        @if (reading().reason; as why) {
          <p class="note"><span>Not available. {{ why }}</span></p>
        } @else if (series(); as s) {
          <pd-sparkline [values]="s.values" [values2]="s.values2" [scale]="sparkScale()" [max]="percent() ? 100 : undefined" [area]="widget().options.fill" [capacity]="sparkCapacity" [formatValue]="sparkFormat()" [pad]="0" [dot]="false" [height]="48" [color]="accent()" color2="var(--text-3)" [label]="title() + ' over the last minute'" />
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
            <span class="rn"><dy-animated-number [value]="reading().text" /><small>{{ reading().unit }}</small></span>
          </dy-ring>
          @if (reading().reason; as why) {
            <p class="note"><span>Not available. {{ why }}</span></p>
          } @else if (batteryState(); as state) {
            <p class="note state">{{ state }}</p>
          }
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
        <div class="issues-scroll" tabindex="0" role="region" aria-label="Unusual things found">
        <ul class="issues">
          @for (a of anomalies(); track a.id) {
            <li>
              <span class="ic" aria-hidden="true"><dy-icon [name]="anomalyIcon(a.kind)" /></span>
              <span class="tx"><strong class="two-lines" [dyFullText]="a.title">{{ a.title }}</strong><small class="truncate" [dyFullText]="label(a.kind) + ' · ' + a.app">{{ label(a.kind) }} · {{ a.app }}</small></span>
            </li>
          }
        </ul>
        </div>
      }
      @case ('cpu') { <dy-cpu-tile /> }
      @case ('diagnosis') { <dy-diagnosis-card /> }
      @case ('events') { <dy-events-feed /> }
      @case ('history') { <dy-history-panel /> }
      @case ('consumers') { <dy-consumers-panel /> }
      @case ('diskTrend') { <dy-disk-trend-panel /> }
      @case ('health') {
        @if (width() >= HERO_MIN_WIDTH) {
          <dy-health-hero [reasons]="rows() >= HERO_ROWS_FOR_THREE ? 3 : 2" />
        } @else {
        @if (health.report(); as r) {
          <div class="ringrow">
            <dy-ring [value]="r.score" [tone]="gradeTone()" [label]="'Health score ' + r.score" style="--ring-size: 84px"><span class="rn num">{{ round(r.score) }}</span></dy-ring>
            <strong class="grade" [attr.data-tone]="gradeTone()">{{ grade() }}</strong>
          </div>
        } @else {
          <p class="note">{{ health.reportError()?.message ?? 'Reading the health score…' }}</p>
        }
        }
      }
    }
    }
  `,
  styles: `
    :host { display: block; min-width: 0; /* The body's height inside its frame: the placement's rows, less the head and the padding. */ --wv-avail: calc(var(--wv-rows, 4) * var(--pd-widget-row) + (var(--wv-rows, 4) - 1) * var(--pd-card-gap) - 22px - 2 * var(--pd-space-3) - var(--pd-widget-grip-clear, 16px)); }
    .twin { display: grid; gap: var(--sp-2); justify-items: start; }
    .sk-big { display: block; font: var(--fw-semibold) var(--fs-2xl) / 1.1 var(--font-display); }
    .sk-line { display: block; font-size: var(--fs-sm); }
    .sk-short { width: 60%; }
    .faint { color: var(--text-3); font-size: var(--fs-xs); }
    .hero[data-tone='warn'] b { color: var(--warn); }
    .hero[data-tone='bad'] b { color: var(--bad); }
    .note { color: var(--text-3); font-size: var(--fs-xs); display: flex; align-items: center; gap: 6px; }
    .note.ok { color: var(--ok); }
    .bar-track { height: 6px; }
    .ringrow { display: flex; align-items: center; gap: var(--sp-4); }
    .rn small { font: var(--fw-regular) var(--fs-sm) / 1 var(--font-sans); margin-inline-start: 4px; color: var(--text-3); }
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
    /* A name wraps to a second line before it is cut (the tooltip has the rest). */
    .two-lines { display: -webkit-box; -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; overflow-wrap: anywhere; }
    /* Every issue is listed; a long list scrolls inside the widget's body and the wheel passes on at its ends. */
    .issues-scroll { max-height: var(--wv-avail); overflow-y: auto; }
    .issues-scroll:focus-visible { outline-offset: -2px; }
    .issues small { color: var(--text-3); font-size: var(--fs-xs); }
  `,
})
export class WidgetView {
  /** The kind this body draws (known before the first value arrives, so the loading twin is the right one). */
  readonly kind = input.required<DashboardKind>();
  /** The placement (its size is the engine's) and the engine's reading of its metric; null while the widget is loading. */
  readonly value = input.required<DyValue | null>();
  protected readonly placement = computed(() => this.value()?.placement ?? NONE);
  protected readonly widget = computed(() => this.placement().widget);
  protected readonly metricReading = computed(() => this.value()?.reading ?? null);
  /** The placement's width in columns (the health widget shows its hero from `HERO_MIN_WIDTH`). */
  protected readonly width = computed(() => this.placement().w);
  protected readonly rows = computed(() => this.placement().h);
  protected readonly HERO_MIN_WIDTH = HERO_MIN_WIDTH;
  protected readonly HERO_ROWS_FOR_THREE = HERO_ROWS_FOR_THREE;
  private readonly choices = inject(WidgetChoicesStore);
  protected readonly live = inject(LiveStore);
  protected readonly health = inject(HealthStore);
  private readonly diagnosis = inject(DiagnosisStore);
  private readonly counters = inject(CountersStore);

  /** The metric's name (the content's accessible names; the frame's head says the same through the board's `titleOf`). */
  protected readonly title = computed(() => this.choices.title(this.widget().metric));
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
  /** The battery ring's state word (the frame's title already says "Battery", so the ring row does not repeat it); null for any other metric. */
  protected readonly batteryState = computed<string | null>(() => {
    if (this.widget().metric !== 'battery') return null;
    const b = this.live.system()?.battery;
    if (!b || !('available' in b)) return null;
    if (b.available.source === 'battery') return 'On battery';
    return b.available.charge >= 99.5 ? 'Full' : 'Charging';
  });
  protected readonly accent = computed(() => this.widget().options.accent ?? 'var(--accent)');
  protected readonly series = computed(() => {
    const path = this.counterPath();
    if (path === null) return seriesFor(this.widget().metric, this.live.series());
    const values = this.counters.trail()[path] ?? [];
    return values.length > 1 ? { values, values2: null } : null;
  });
  /** Whether the widget shows a thermal reading, whose source (the helper, or why nobody could read it) the engine reports with each update. */
  protected readonly thermal = computed(() => this.widget().metric === 'temperature');
  protected readonly thermalVia = computed(() => this.live.update()?.via?.temperatures);
  protected readonly cores = computed(() => this.live.system()?.cpu.perCore ?? []);
  protected readonly busiest = computed(() => Math.max(0, ...this.cores()));
  protected readonly top = computed(() => {
    const u = this.live.update();
    return ((this.widget().metric === 'memory' ? u?.topMemory : u?.topCpu) ?? []).slice(0, 5);
  });
  /** Every issue the engine found. */
  protected readonly anomalies = this.diagnosis.anomalies;
  protected readonly grade = computed(() => (this.health.report() ? GRADE_LABEL[this.health.report()!.grade] : ''));
  protected readonly gradeTone = computed(() => (this.health.report() ? GRADE_TONE[this.health.report()!.grade] : 'accent'));
  protected readonly pct = formatPercent;
  protected readonly sparkCapacity = SPARK_CAPACITY;
  protected readonly sparkScale = computed(() => (this.zoomed() && this.percent() ? 'zoom' : 'zero') as 'zoom' | 'zero');
  /** The graph's values in words for its accessible name: a percentage, a rate, or a number with the reading's unit. */
  protected readonly sparkFormat = computed<(v: number) => string>(() => {
    if (this.percent()) return formatPercent;
    const m = this.widget().metric;
    if (m === 'networkDown' || m === 'networkUp' || m === 'networkBoth') return formatRate;
    const unit = this.reading().unit;
    return (v) => `${formatNumber(v)} ${unit}`.trim();
  });
  protected readonly round = (v: number) => Math.round(v).toString();
  protected readonly label = (k: keyof typeof ANOMALY_LABEL) => ANOMALY_LABEL[k];
  protected readonly anomalyIcon = (k: keyof typeof ANOMALY_ICON) => ANOMALY_ICON[k];
  /** Slow-moving levels are drawn zoomed so a steady 50% still shows its line. */
  protected readonly zoomed = computed(() => ['memory', 'swap', 'gpuLoad', 'diskFree', 'battery'].includes(String(this.widget().metric)));
  protected readonly topValue = (g: { cpu: Measured<number>; memory: Measured<number> }) => (this.widget().metric === 'memory' ? showMeasured(g.memory, formatBytes) : showMeasured(g.cpu, formatPercent));
  protected readonly cellColor = coreHeat;
}
