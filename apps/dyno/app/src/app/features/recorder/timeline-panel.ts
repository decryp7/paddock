import { ChangeDetectionStrategy, Component, computed, inject, input, linkedSignal, output, signal } from '@angular/core';
import type { RecorderSnapshot } from '../../core/api.types';
import { ThemeService } from '../../core/theme';
import { Chart } from '../../shared/chart';
import { formatBytes } from '../../shared/format';
import { clockSeconds, momentInRange, overviewPoints, panelHeadings, rangeFromDrag, timelineOption, timelineSummary, TIMELINE_HEIGHT, type SampleRange } from './recorder-options';
import { MatButton } from '@angular/material/button';
import { MatTooltip } from '@angular/material/tooltip';

/** One snapshot's timeline (stacked charts on one time axis) and the busiest processes at the picked moment. */
@Component({
  selector: 'dy-timeline-panel',
  imports: [MatTooltip, MatButton, Chart],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="plot" tabindex="0" role="group" aria-label="Timeline. Left and right arrow keys move the picked moment." (keydown)="onKey($event)">
      @for (h of headings(); track h.title) {
        <div class="heading" [style.top.px]="h.top">
          <span class="title">{{ h.title }}</span>
          @for (k of h.keys; track k.label) {
            <span class="key"><i [class.dashed]="k.dashed" [style.--c]="k.series === 'a' ? colorA() : colorB()"></i>{{ k.label }}</span>
          }
          <span class="value">{{ h.value }}</span>
        </div>
      }
      <dy-chart [options]="option()" [label]="summary()" [style.--chart-h.px]="height" (itemClick)="onClick($event)" />
    </div>
    <div class="brush-row">
      <div class="brush" role="group" aria-label="Zoom range. Drag across the strip to zoom the timeline to that stretch."
        (pointerdown)="brushStart($event)" (pointermove)="brushMove($event)" (pointerup)="brushEnd($event)" (pointercancel)="drag.set(null)">
        <svg viewBox="0 0 100 100" preserveAspectRatio="none" aria-hidden="true"><polyline [attr.points]="overview()" /></svg>
        @if (shown(); as r) {
          <i class="sel" [style.left.%]="r.left" [style.width.%]="r.width"></i>
        }
      </div>
      <span class="range-text">{{ rangeText() }}</span>
      @if (range()) {
        <button type="button" matButton="text" (click)="clearRange()">Show all</button>
      }
    </div>
    <section class="busy" aria-label="Who was busiest">
      <div class="busy-head">
        <h2>Who was busiest</h2>
        <span class="at">{{ momentTime() }} · CPU {{ cpuAt() }}</span>
        <button type="button" matButton="text" (click)="peak.emit()">Jump to peak</button>
      </div>
      <table class="table">
        <caption class="visually-hidden">Top processes at {{ momentTime() }}</caption>
        <thead><tr><th>Process</th><th class="r">CPU, % of machine</th><th class="r">Memory</th></tr></thead>
        <tbody>
          @for (p of top(); track p.name) {
            <tr><td class="name" [matTooltip]="p.name">{{ p.name }}</td><td class="r">{{ p.cpu.toFixed(1) }}</td><td class="r">{{ bytes(p.memory) }}</td></tr>
          } @empty {
            <tr><td colspan="3" class="muted">No process detail was recorded for this second.</td></tr>
          }
        </tbody>
      </table>
    </section>
  `,
  styles: `
    :host { display: grid; gap: var(--sp-4); min-width: 0; align-content: start; }
    .plot { position: relative; border-radius: var(--r-md); }
    .heading { position: absolute; left: 0; right: 0; height: 20px; display: flex; align-items: center; gap: var(--sp-3); pointer-events: none; z-index: 1; font-size: var(--fs-sm); white-space: nowrap; }
    .title { color: var(--text-2); font-weight: var(--fw-medium); }
    .value { color: var(--text-1); font-weight: var(--fw-semibold); font-variant-numeric: tabular-nums; }
    .key { display: inline-flex; align-items: center; gap: var(--sp-1); color: var(--text-3); }
    .key i { width: 16px; height: 0; border-top: 2px solid var(--c); }
    .key i.dashed { border-top-style: dashed; }
    .brush-row { display: flex; align-items: center; gap: var(--sp-3); margin-top: calc(var(--sp-2) * -1); }
    .brush { position: relative; flex: 1; height: 28px; border: 1px solid var(--border); border-radius: var(--r-sm); background: var(--surface-2); cursor: col-resize; touch-action: none; overflow: hidden; }
    .brush svg { position: absolute; inset: 0; width: 100%; height: 100%; }
    .brush polyline { fill: none; stroke: var(--text-3); stroke-width: 1; vector-effect: non-scaling-stroke; }
    .brush .sel { position: absolute; top: 0; bottom: 0; background: var(--accent); opacity: 0.22; border-inline: 1px solid var(--accent); pointer-events: none; }
    .range-text { color: var(--text-3); font-size: var(--fs-sm); white-space: nowrap; font-variant-numeric: tabular-nums; }
    .plot:focus-visible { outline: 2px solid var(--focus); outline-offset: 2px; }
    .busy-head { display: flex; align-items: center; gap: var(--sp-3); margin-bottom: var(--sp-2); }
    .busy-head h2 { margin: 0; white-space: nowrap; font-size: var(--fs-md); font-weight: var(--fw-semibold); }
    .at { white-space: nowrap; color: var(--text-2); font-size: var(--fs-sm); font-variant-numeric: tabular-nums; }
    .busy-head .mat-mdc-button-base { margin-left: auto; }
    .table { width: 100%; }
    .r { width: 150px; text-align: right; white-space: nowrap; font-variant-numeric: tabular-nums; }
    .name { max-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .muted { color: var(--text-3); }
  `,
})
export class TimelinePanel {
  readonly snapshot = input.required<RecorderSnapshot>();
  readonly moment = input.required<number>();
  readonly picked = output<number>();
  readonly peak = output<void>();
  readonly step = output<number>();

  private readonly theme = inject(ThemeService);
  protected readonly height = TIMELINE_HEIGHT;
  protected readonly bytes = formatBytes;
  protected readonly option = computed(() => timelineOption(this.snapshot().samples, this.moment(), this.theme.tokens(), this.range()));
  protected readonly summary = computed(() => timelineSummary(this.snapshot().samples));
  protected readonly colorA = computed(() => this.theme.tokens().chart[2]!);
  protected readonly colorB = computed(() => this.theme.tokens().chart[4]!);
  protected readonly headings = computed(() => panelHeadings(this.sample()));
  private readonly sample = computed(() => this.snapshot().samples[this.moment()]);
  protected readonly top = computed(() => this.sample()?.top ?? []);
  protected readonly momentTime = computed(() => clockSeconds(this.sample()?.at ?? this.snapshot().at));
  protected readonly cpuAt = computed(() => `${(this.sample()?.cpuTotal ?? 0).toFixed(1)}%`);

  /** The zoomed stretch, or null for the whole snapshot. */
  protected readonly range = linkedSignal<number, SampleRange | null>({ source: () => this.snapshot().at, computation: () => null });
  /** The drag in progress, as pixel offsets inside the strip. */
  protected readonly drag = signal<{ x0: number; x1: number; width: number } | null>(null);
  protected readonly overview = computed(() => overviewPoints(this.snapshot().samples));
  /** The strip's highlighted part (percent): the drag while it goes on, else the zoomed range. */
  protected readonly shown = computed(() => {
    const n = this.snapshot().samples.length;
    const d = this.drag();
    if (d && d.width > 0) {
      const a = Math.min(d.x0, d.x1);
      return { left: (a / d.width) * 100, width: (Math.abs(d.x1 - d.x0) / d.width) * 100 };
    }
    const r = this.range();
    return r && n > 1 ? { left: (r.from / (n - 1)) * 100, width: ((r.to - r.from) / (n - 1)) * 100 } : null;
  });
  protected readonly rangeText = computed(() => {
    const s = this.snapshot().samples;
    const r = this.range();
    const a = s[r?.from ?? 0];
    const b = s[r?.to ?? s.length - 1];
    return a && b ? `${clockSeconds(a.at)} to ${clockSeconds(b.at)}` : '';
  });

  protected brushStart(e: PointerEvent): void {
    const el = e.currentTarget as HTMLElement;
    const box = el.getBoundingClientRect();
    el.setPointerCapture?.(e.pointerId);
    const x = e.clientX - box.left;
    this.drag.set({ x0: x, x1: x, width: box.width });
  }

  protected brushMove(e: PointerEvent): void {
    const d = this.drag();
    if (!d) return;
    const box = (e.currentTarget as HTMLElement).getBoundingClientRect();
    this.drag.set({ ...d, x1: Math.min(d.width, Math.max(0, e.clientX - box.left)) });
  }

  protected brushEnd(e: PointerEvent): void {
    this.brushMove(e);
    const d = this.drag();
    this.drag.set(null);
    if (!d) return;
    const r = rangeFromDrag(d.x0, d.x1, d.width, this.snapshot().samples.length);
    if (!r) return;
    this.range.set(r);
    const m = momentInRange(this.moment(), r);
    if (m !== this.moment()) this.picked.emit(m);
  }

  protected clearRange(): void {
    this.range.set(null);
  }

  protected onClick(p: { name?: string }): void {
    const i = Number(p.name);
    if (p.name !== undefined && Number.isInteger(i)) this.picked.emit(i);
  }

  protected onKey(e: KeyboardEvent): void {
    const d = e.key === 'ArrowLeft' ? -1 : e.key === 'ArrowRight' ? 1 : 0;
    if (d === 0) return;
    e.preventDefault();
    this.step.emit(e.shiftKey ? d * 10 : d);
  }
}
