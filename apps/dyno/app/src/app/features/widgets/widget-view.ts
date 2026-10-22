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
import { ANOMALY_LABEL } from '../../shared/labels';
import type { Widget, WidgetKind } from '../../core/api.types';
import { AnimatedNumber } from './animated-number';
import { KIND_LABEL, metricLabel, readMetric, seriesFor, thresholdTone } from './widget-model';

const ICON: Record<WidgetKind, IconName> = {
  graph: 'trend',
  bigNumber: 'gauge',
  bars: 'reports',
  ring: 'gauge',
  heatmap: 'cpu',
  topProcesses: 'processes',
  issues: 'alert',
  health: 'health',
};

/**
 * One dashboard widget, drawn from its configuration: a graph, big number, bars, ring, per-core heatmap, top processes,
 * issues or the health score. Neutral by default; the hero uses the accent, and a threshold colour only appears when a
 * threshold is crossed (always with the number printed). Values glide to their new reading.
 */
@Component({
  selector: 'dy-widget',
  imports: [Icon, Ring, Sparkline, AnimatedNumber],
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: { role: 'group', '[attr.aria-label]': 'title()', '[attr.data-kind]': 'widget().kind' },
  template: `
    <header>
      <dy-icon [name]="icon()" />
      <h3 class="truncate">{{ title() }}</h3>
      @if (reading().reason && widget().kind !== 'topProcesses' && widget().kind !== 'health' && widget().kind !== 'issues') {
        <span class="faint">Not available</span>
      }
    </header>
    @switch (widget().kind) {
      @case ('bigNumber') {
        <div class="hero" [attr.data-tone]="tone()"><b><dy-animated-number [value]="reading().text[0]" /></b><small>{{ reading().text[1] }}</small></div>
        @if (reading().reason; as why) {
          <p class="note">{{ why }}</p>
        }
      }
      @case ('graph') {
        <div class="hero" [attr.data-tone]="tone()"><b><dy-animated-number [value]="reading().text[0]" /></b><small>{{ reading().text[1] }}</small></div>
        @if (series(); as s) {
          <dy-sparkline [values]="s.values" [values2]="s.values2" [max]="percent() ? 100 : null" [fill]="widget().options.fill" [color]="accent()" color2="var(--text-3)" [label]="title() + ' over the last minute'" style="--spark-h: 48px" />
        } @else {
          <p class="note">{{ reading().reason ?? 'No live history is kept for this metric.' }}</p>
        }
      }
      @case ('bars') {
        <div class="hero" [attr.data-tone]="tone()"><b><dy-animated-number [value]="reading().text[0]" /></b><small>{{ reading().text[1] }}</small></div>
        <div class="bar-track" [attr.data-tone]="barTone()" role="img" [attr.aria-label]="title() + ' ' + reading().text.join(' ')"><span [style.width.%]="reading().percent ?? 0"></span></div>
      }
      @case ('ring') {
        <div class="ringrow">
          <dy-ring [value]="reading().percent ?? 0" [tone]="ringTone()" [label]="title() + ' ' + reading().text.join('')" style="--ring-size: 84px">
            <span class="rn"><dy-animated-number [value]="reading().text[0]" /></span>
          </dy-ring>
          <p class="note">{{ reading().reason ?? title() }}</p>
        </div>
      }
      @case ('heatmap') {
        <div class="cells" role="img" [attr.aria-label]="'Per-core load: ' + cores().map(pct).join(', ')">
          @for (v of cores(); track $index) {
            <span class="cell" [style.background]="cellColor(v)" [title]="'Core ' + round($index + 1) + ': ' + pct(v)"></span>
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
        <ul>
          @for (a of anomalies(); track a.id) {
            <li><b>{{ label(a.kind) }}</b> <span class="truncate">{{ a.title }}</span></li>
          }
        </ul>
      }
      @case ('health') {
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
  `,
  styles: `
    :host { display: flex; flex-direction: column; gap: var(--sp-3); min-width: 0; min-height: 132px; padding: var(--sp-4); background: var(--surface-2); border: 1px solid var(--border); border-radius: var(--r-lg); box-shadow: var(--shadow-1); }
    header { display: flex; align-items: center; gap: var(--sp-2); color: var(--text-2); }
    header dy-icon { --icon-size: 16px; color: var(--text-3); }
    h3 { font-size: var(--fs-sm); font-weight: var(--fw-semibold); color: var(--text-2); flex: 1; }
    .faint { color: var(--text-3); font-size: var(--fs-xs); }
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
    ul li b { font-weight: var(--fw-medium); }
  `,
})
export class WidgetView {
  readonly widget = input.required<Widget>();
  protected readonly live = inject(LiveStore);
  protected readonly health = inject(HealthStore);
  private readonly diagnosis = inject(DiagnosisStore);

  protected readonly title = computed(() => (this.widget().kind === 'topProcesses' ? `Top apps by ${metricLabel(this.widget().metric).toLowerCase()}` : this.widget().kind === 'health' || this.widget().kind === 'issues' ? KIND_LABEL[this.widget().kind] : metricLabel(this.widget().metric)));
  protected readonly icon = computed(() => ICON[this.widget().kind]);
  protected readonly reading = computed(() => readMetric(this.widget().metric, this.live.system()));
  protected readonly percent = computed(() => this.reading().percent !== null);
  protected readonly tone = computed(() => thresholdTone(this.reading().value, this.widget().thresholds));
  protected readonly barTone = computed(() => (this.tone() === 'ok' ? null : this.tone()));
  protected readonly ringTone = computed<'ok' | 'warn' | 'bad' | 'accent'>(() => (this.tone() === 'ok' ? 'accent' : this.tone()));
  protected readonly accent = computed(() => this.widget().options.accent ?? 'var(--accent)');
  protected readonly series = computed(() => seriesFor(this.widget().metric, this.live.series()));
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
  protected readonly topValue = (g: { cpu: number; memory: number }) => (this.widget().metric === 'memory' ? formatBytes(g.memory) : formatPercent(g.cpu));
  protected readonly cellColor = (v: number) => `color-mix(in srgb, var(--accent) ${Math.round(Math.min(100, Math.max(0, v)) * 0.85)}%, var(--surface-active))`;
}
