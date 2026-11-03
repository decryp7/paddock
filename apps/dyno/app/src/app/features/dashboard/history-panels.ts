import { barValue, showMeasured } from '../../core/measured';
import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';
import type { HistoryRange, TimelineEventKind } from '../../core/api.types';
import { ThemeService } from '../../core/theme';
import { Chart } from '../../shared/chart';
import { AsyncState } from '../../shared/async-state';
import { Icon } from '../../shared/icon';
import { EVENT_LABEL, forecastText } from '../../shared/labels';
import { formatBytes, formatPercent } from '../../shared/format';
import { LiveStore } from '../../state/live.store';
import { HistoryStore } from '../../state/history.store';
import { EVENT_STYLE, consumersOption, diskTrendOption, eventCounts, historyOption } from './history-options';
import { MatButton, MatIconButton } from '@angular/material/button';
import { SuSegmentedControl, type SuSegment } from '@decryp7/paddock-ui';
import { MatTooltip } from '@angular/material/tooltip';
import { MatCheckbox } from '@angular/material/checkbox';

export const RANGES: { id: HistoryRange; label: string; long: string }[] = [
  { id: 'hour', label: '1h', long: 'Last hour' },
  { id: 'day', label: '24h', long: 'Last 24 hours' },
  { id: 'week', label: '7d', long: 'Last 7 days' },
  { id: 'month', label: '30d', long: 'Last 30 days' },
];

/** The glyph that stands for an event shape in the legend (the chart uses the same shapes). */
const GLYPH: Record<string, string> = { triangle: '▲', rect: '■', circle: '●', diamond: '◆', pin: '◉', roundRect: '▬', arrow: '➤', emptyCircle: '○' };

/** CPU and memory history with the events of the range marked on it, a range switch and a legend that filters the events. */
@Component({
  selector: 'dy-history-panel',
  imports: [MatButton, MatCheckbox, MatTooltip, SuSegmentedControl, MatIconButton, Chart, AsyncState, Icon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="tools">
      <pd-segmented-control label="Range" [options]="rangeSegs" [value]="store.range()" (valueChange)="pickRange($event)" />
      <button type="button" matIconButton matTooltip="Refresh history" aria-label="Refresh history" (click)="store.load()"><dy-icon name="refresh" /></button>
      @if (zoomed()) {
        <button type="button" matButton="text" (click)="resetZoom()"><dy-icon name="undo" /> Reset zoom</button>
      }
    </div>
    <dy-async [loaded]="!!store.history()" [error]="store.error()" [empty]="empty()" emptyTitle="No history yet" emptyText="Dyno keeps what it measures; the chart fills in as time passes." what="history" [rows]="0" (retry)="store.load()">
      <div skeleton class="skeleton chart-skeleton"></div>
      <dy-chart [options]="option()" [label]="chartLabel()" (zoomed)="zoomed.set(true)" style="--chart-h: calc(var(--wv-avail, 320px) - 96px)" />
      <div class="legend" role="group" aria-label="Events on the timeline">
        @for (e of events(); track e.kind) {
          <mat-checkbox [checked]="!hidden().has(e.kind)" (change)="toggle(e.kind)">
            <span class="glyph" [style.color]="color(e.kind)" aria-hidden="true">{{ glyph(e.kind) }}</span>
            {{ label(e.kind) }} <span class="n num">{{ e.count }}</span>
          </mat-checkbox>
        } @empty {
          <span class="faint">No alerts, restarts or network changes in this range.</span>
        }
      </div>
    </dy-async>
  `,
  styles: `
    :host { display: grid; gap: var(--sp-2); min-width: 0; align-content: start; }
    .tools { display: flex; align-items: center; gap: var(--sp-2); flex-wrap: wrap; }
    .chart-skeleton { height: 200px; }
    .legend { display: flex; flex-wrap: wrap; gap: var(--sp-2); }
    .glyph { font-size: 10px; line-height: 1; }
    .n { color: var(--text-3); }
    .faint { color: var(--text-3); font-size: var(--fs-xs); }
  `,
})
export class HistoryPanel {
  protected readonly store = inject(HistoryStore);
  private readonly theme = inject(ThemeService);
  protected readonly ranges = RANGES;
  protected readonly rangeSegs: SuSegment[] = RANGES.map((r) => ({ value: r.id, label: r.label }));
  protected pickRange(value: string): void {
    const range = RANGES.find((r) => r.id === value);
    if (range) void this.store.setRange(range.id);
  }
  protected readonly hidden = signal<ReadonlySet<TimelineEventKind>>(new Set());
  protected readonly empty = computed(() => (this.store.history()?.points.length ?? 0) === 0);
  protected readonly events = computed(() => eventCounts(this.store.history()?.events ?? []));
  protected readonly zoomed = signal(false);
  private readonly resetKey = signal(0);
  protected readonly option = computed(() => {
    this.resetKey();
    const h = this.store.history();
    return h && h.points.length ? historyOption(h, this.theme.tokens(), this.hidden()) : null;
  });
  protected readonly chartLabel = computed(() => `CPU and memory over the ${RANGES.find((r) => r.id === this.store.range())?.long.toLowerCase() ?? 'range'}`);
  protected readonly label = (k: TimelineEventKind) => EVENT_LABEL[k];
  protected readonly glyph = (k: TimelineEventKind) => GLYPH[EVENT_STYLE[k].symbol] ?? '●';
  protected readonly color = (k: TimelineEventKind) => EVENT_STYLE[k].color(this.theme.tokens());

  /** A fresh option object redraws the chart at full range. */
  protected resetZoom(): void {
    this.zoomed.set(false);
    this.resetKey.update((k) => k + 1);
  }

  protected toggle(kind: TimelineEventKind): void {
    this.hidden.update((h) => {
      const next = new Set(h);
      if (!next.delete(kind)) next.add(kind);
      return next;
    });
  }
}

/** Free space of a drive over time with the engine's "full in N days" forecast drawn as a dashed projection. */
@Component({
  selector: 'dy-disk-trend-panel',
  imports: [SuSegmentedControl, Chart, AsyncState],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @if (trend(); as t) {
      <div class="tools">
        @if (trends().length > 1) {
          <pd-segmented-control label="Drive" [options]="driveSegs()" [value]="index() + ''" (valueChange)="index.set(+$event)" />
        }
        <span class="spacer"></span>
        <span class="badge" [attr.data-tone]="tone()">{{ forecast() }}</span>
      </div>
    }
    <dy-async [loaded]="!!store.history()" [error]="store.error()" [empty]="trends().length === 0" emptyTitle="No drives measured" emptyText="Free space is recorded once the agent has run for a while." what="disk trend" [rows]="0" (retry)="store.load()">
      <div skeleton class="skeleton chart-skeleton"></div>
      @if (trend(); as t) {
        <dy-chart [options]="option()" [label]="'Free space on ' + t.disk + ' over time. ' + forecast()" style="--chart-h: calc(var(--wv-avail, 200px) - 70px)" />
        <p class="foot num">{{ bytes(last()) }} free of {{ bytes(t.total) }}</p>
      }
    </dy-async>
  `,
  styles: `
    :host { display: grid; gap: var(--sp-2); min-width: 0; align-content: start; }
    .tools { display: flex; align-items: center; gap: var(--sp-2); flex-wrap: wrap; }
    .spacer { flex: 1; }
    .chart-skeleton { height: 120px; }
    dy-async { display: grid; gap: var(--sp-2); }
    .foot { color: var(--text-3); font-size: var(--fs-xs); }
  `,
})
export class DiskTrendPanel {
  protected readonly store = inject(HistoryStore);
  private readonly theme = inject(ThemeService);
  protected readonly index = signal(0);
  protected readonly trends = computed(() => this.store.history()?.diskTrends ?? []);
  protected readonly driveSegs = computed<SuSegment[]>(() => this.trends().map((t, i) => ({ value: String(i), label: t.disk })));
  protected readonly trend = computed(() => this.trends()[this.index()] ?? this.trends()[0] ?? null);
  protected readonly forecast = computed(() => (this.trend() ? forecastText(this.trend()!.forecast) : ''));
  protected readonly tone = computed(() => (typeof this.trend()?.forecast === 'object' ? 'warn' : 'ok'));
  protected readonly option = computed(() => (this.trend() ? diskTrendOption(this.trend()!, this.theme.tokens()) : null));
  protected readonly last = computed(() => this.trend()?.points.at(-1)?.free ?? 0);
  protected readonly bytes = formatBytes;
}

/** The apps that used the most CPU over the range. */
@Component({
  selector: 'dy-consumers-panel',
  imports: [SuSegmentedControl, Chart, AsyncState],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="tools">
      <span class="spacer"></span>
      <pd-segmented-control label="Top consumers period" [options]="periodSegs()" [value]="view()" (valueChange)="view.set($event === 'now' ? 'now' : 'range')" />
    </div>
    @if (view() === 'now') {
      <ol class="now" aria-label="Busiest apps right now">
        @for (g of now(); track g.id) {
          <li><span class="truncate">{{ g.name }}</span><span class="bar" role="presentation"><i [style.width.%]="bar(g.cpu)"></i></span><span class="num">{{ show(g.cpu, pct) }}</span></li>
        } @empty {
          <li class="faint">Waiting for the first reading…</li>
        }
      </ol>
    } @else {
      <dy-async [loaded]="!!store.history()" [error]="store.error()" [empty]="consumers().length === 0" emptyTitle="Nothing to rank yet" what="top consumers" [rows]="0" (retry)="store.load()">
        <div skeleton class="skeleton chart-skeleton"></div>
        <dy-chart [options]="option()" label="Apps with the highest average CPU over the range" style="--chart-h: calc(var(--wv-avail, 200px) - 44px)" />
      </dy-async>
    }
  `,
  styles: `
    :host { display: grid; gap: var(--sp-2); min-width: 0; align-content: start; }
    .tools { display: flex; align-items: center; gap: var(--sp-2); }
    .spacer { flex: 1; }
    .chart-skeleton { height: 120px; }
    .faint { color: var(--text-3); font-size: var(--fs-xs); }
    .now { list-style: none; margin: 0; padding: 0; display: grid; gap: var(--sp-2); font-size: var(--fs-sm); }
    .now li { display: grid; grid-template-columns: minmax(0, 1fr) 90px 52px; gap: var(--sp-3); align-items: center; }
    .now .bar { height: 5px; border-radius: 3px; background: var(--surface-active); overflow: hidden; }
    .now .bar i { display: block; height: 100%; background: var(--accent); min-width: 2px; }
    .now .num { text-align: right; }
  `,
})
export class ConsumersPanel {
  protected readonly store = inject(HistoryStore);
  private readonly theme = inject(ThemeService);
  protected readonly consumers = computed(() => this.store.history()?.topConsumers ?? []);
  protected readonly option = computed(() => (this.consumers().length ? consumersOption(this.consumers(), this.theme.tokens()) : null));
  protected readonly rangeShort = computed(() => RANGES.find((r) => r.id === this.store.range())?.label ?? '24h');
  protected readonly view = signal<'now' | 'range'>('range');
  protected readonly periodSegs = computed<SuSegment[]>(() => [{ value: 'now', label: 'Now' }, { value: 'range', label: this.rangeShort() }]);
  private readonly live = inject(LiveStore);
  protected readonly now = computed(() => (this.live.update()?.topCpu ?? []).slice(0, 6));
  protected readonly pct = formatPercent;
  protected readonly show = showMeasured;
  protected readonly bar = barValue;
}
