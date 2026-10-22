import { ChangeDetectionStrategy, Component, OnInit, computed, inject, signal } from '@angular/core';
import type { HistoryRange, TimelineEventKind } from '../../core/api.types';
import { ThemeService } from '../../core/theme';
import { Chart } from '../../shared/chart';
import { AsyncState } from '../../shared/async-state';
import { Icon } from '../../shared/icon';
import { EVENT_LABEL, forecastText } from '../../shared/labels';
import { formatBytes } from '../../shared/format';
import { HistoryStore } from '../../state/history.store';
import { EVENT_STYLE, consumersOption, diskTrendOption, eventCounts, historyOption } from './history-options';

export const RANGES: { id: HistoryRange; label: string; long: string }[] = [
  { id: 'hour', label: '1h', long: 'Last hour' },
  { id: 'day', label: '24h', long: 'Last 24 hours' },
  { id: 'week', label: '7d', long: 'Last 7 days' },
  { id: 'month', label: '30d', long: 'Last 30 days' },
];

/** The glyph that stands for an event shape in the legend (the chart uses the same shapes). */
const GLYPH: Record<string, string> = { triangle: '▲', rect: '■', circle: '●', diamond: '◆', pin: '◉', roundRect: '▬', arrow: '➤' };

/** CPU and memory history with the events of the range marked on it, a range switch and a legend that filters the events. */
@Component({
  selector: 'dy-history-panel',
  imports: [Chart, AsyncState, Icon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <section class="card" aria-label="History">
      <div class="card-head">
        <h2><dy-icon name="trend" /> History</h2>
        <span class="spacer"></span>
        <div class="seg" role="group" aria-label="Range">
          @for (r of ranges; track r.id) {
            <button type="button" [attr.aria-pressed]="store.range() === r.id" [title]="r.long" (click)="store.setRange(r.id)">{{ r.label }}</button>
          }
        </div>
        <button type="button" class="btn ghost icon sm" aria-label="Refresh history" (click)="store.load()"><dy-icon name="refresh" /></button>
      </div>
      <dy-async [loaded]="!!store.history()" [error]="store.error()" [empty]="empty()" emptyTitle="No history yet" emptyText="Dyno keeps what it measures; the chart fills in as time passes." what="history" [rows]="0" (retry)="store.load()">
        <div skeleton class="skeleton chart-skeleton"></div>
        @if (zoomed()) {
          <button type="button" class="chip-toggle reset" (click)="resetZoom()"><dy-icon name="undo" /> Reset zoom</button>
        }
        <dy-chart [options]="option()" [label]="chartLabel()" (zoomed)="zoomed.set(true)" style="--chart-h: 430px" />
        <div class="legend" role="group" aria-label="Events on the timeline">
          @for (e of events(); track e.kind) {
            <button type="button" class="chip-toggle" [attr.aria-pressed]="!hidden().has(e.kind)" (click)="toggle(e.kind)" [title]="'Show or hide: ' + label(e.kind)">
              <span class="glyph" [style.color]="color(e.kind)" aria-hidden="true">{{ glyph(e.kind) }}</span>
              {{ label(e.kind) }} <span class="n num">{{ e.count }}</span>
            </button>
          } @empty {
            <span class="faint">No rule actions, cleanups or alerts in this range.</span>
          }
        </div>
      </dy-async>
    </section>
  `,
  styles: `
    :host { display: block; min-width: 0; }
    .card { height: 100%; padding-bottom: var(--sp-3); }
    .card-head { padding-bottom: var(--sp-1); }
    dy-async { padding: 0 var(--sp-4); }
    .chart-skeleton { height: 430px; margin-top: var(--sp-2); }
    .reset { justify-self: start; margin-bottom: var(--sp-1); }
    .legend { display: flex; flex-wrap: wrap; gap: var(--sp-2); padding-top: var(--sp-1); }
    .chip-toggle { display: inline-flex; align-items: center; gap: 6px; height: 24px; padding: 0 var(--sp-2); border: 1px solid var(--border); border-radius: var(--r-pill); background: var(--surface-inset); color: var(--text-2); font: var(--fs-xs) var(--font-sans); cursor: default; }
    .chip-toggle[aria-pressed='false'] { opacity: 0.5; text-decoration: line-through; }
    .chip-toggle:hover { border-color: var(--border-strong); }
    .glyph { font-size: 10px; line-height: 1; }
    .n { color: var(--text-3); }
    .faint { color: var(--text-3); font-size: var(--fs-xs); }
  `,
})
export class HistoryPanel implements OnInit {
  protected readonly store = inject(HistoryStore);
  private readonly theme = inject(ThemeService);
  protected readonly ranges = RANGES;
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

  ngOnInit(): void {
    void this.store.ensure();
  }

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
  imports: [Chart, AsyncState, Icon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <section class="card" aria-label="Disk space trend">
      <div class="card-head">
        <h2><dy-icon name="disk" /> Free disk space</h2>
        <span class="spacer"></span>
        @if (trend(); as t) {
          <span class="badge" [attr.data-tone]="tone()">{{ forecast() }}</span>
        }
      </div>
      <dy-async [loaded]="!!store.history()" [error]="store.error()" [empty]="trends().length === 0" emptyTitle="No drives measured" emptyText="Free space is recorded once the agent has run for a while." what="disk trend" [rows]="0" (retry)="store.load()">
        <div skeleton class="skeleton chart-skeleton"></div>
        @if (trends().length > 1) {
          <div class="seg" role="group" aria-label="Drive">
            @for (t of trends(); track t.disk; let i = $index) {
              <button type="button" [attr.aria-pressed]="i === index()" (click)="index.set(i)">{{ t.disk }}</button>
            }
          </div>
        }
        @if (trend(); as t) {
          <dy-chart [options]="option()" [label]="'Free space on ' + t.disk + ' over time. ' + forecast()" style="--chart-h: 190px" />
          <p class="foot num">{{ bytes(last()) }} free of {{ bytes(t.total) }}</p>
        }
      </dy-async>
    </section>
  `,
  styles: `
    :host { display: block; min-width: 0; }
    .card { height: 100%; padding-bottom: var(--sp-3); }
    dy-async { padding: 0 var(--sp-4); display: grid; gap: var(--sp-2); }
    .chart-skeleton { height: 210px; margin-top: var(--sp-2); }
    .seg { justify-self: start; margin-top: var(--sp-1); }
    .foot { color: var(--text-3); font-size: var(--fs-xs); }
  `,
})
export class DiskTrendPanel implements OnInit {
  protected readonly store = inject(HistoryStore);
  private readonly theme = inject(ThemeService);
  protected readonly index = signal(0);
  protected readonly trends = computed(() => this.store.history()?.diskTrends ?? []);
  protected readonly trend = computed(() => this.trends()[this.index()] ?? this.trends()[0] ?? null);
  protected readonly forecast = computed(() => (this.trend() ? forecastText(this.trend()!.forecast) : ''));
  protected readonly tone = computed(() => (typeof this.trend()?.forecast === 'object' ? 'warn' : 'ok'));
  protected readonly option = computed(() => (this.trend() ? diskTrendOption(this.trend()!, this.theme.tokens()) : null));
  protected readonly last = computed(() => this.trend()?.points.at(-1)?.free ?? 0);
  protected readonly bytes = formatBytes;

  ngOnInit(): void {
    void this.store.ensure();
  }
}

/** The apps that used the most CPU over the range. */
@Component({
  selector: 'dy-consumers-panel',
  imports: [Chart, AsyncState, Icon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <section class="card" aria-label="Top consumers">
      <div class="card-head">
        <h2><dy-icon name="processes" /> Top consumers</h2>
        <span class="spacer"></span>
        <span class="faint">{{ rangeText() }}</span>
      </div>
      <dy-async [loaded]="!!store.history()" [error]="store.error()" [empty]="consumers().length === 0" emptyTitle="Nothing to rank yet" what="top consumers" [rows]="0" (retry)="store.load()">
        <div skeleton class="skeleton chart-skeleton"></div>
        <dy-chart [options]="option()" label="Apps with the highest average CPU over the range" style="--chart-h: 190px" />
      </dy-async>
    </section>
  `,
  styles: `
    :host { display: block; min-width: 0; }
    .card { height: 100%; padding-bottom: var(--sp-3); }
    dy-async { padding: 0 var(--sp-4); }
    .chart-skeleton { height: 190px; margin-top: var(--sp-2); }
    .faint { color: var(--text-3); font-size: var(--fs-xs); }
  `,
})
export class ConsumersPanel implements OnInit {
  protected readonly store = inject(HistoryStore);
  private readonly theme = inject(ThemeService);
  protected readonly consumers = computed(() => this.store.history()?.topConsumers ?? []);
  protected readonly option = computed(() => (this.consumers().length ? consumersOption(this.consumers(), this.theme.tokens()) : null));
  protected readonly rangeText = computed(() => RANGES.find((r) => r.id === this.store.range())?.long ?? '');

  ngOnInit(): void {
    void this.store.ensure();
  }
}
