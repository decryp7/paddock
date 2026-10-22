import { ChangeDetectionStrategy, Component, computed, inject, input, output } from '@angular/core';
import type { RecorderSnapshot } from '../../core/api.types';
import { ThemeService } from '../../core/theme';
import { Chart } from '../../shared/chart';
import { formatBytes } from '../../shared/format';
import { clockSeconds, panelHeadings, timelineOption, timelineSummary, TIMELINE_HEIGHT } from './recorder-options';

/** One snapshot's timeline (stacked charts on one time axis) and the busiest processes at the picked moment. */
@Component({
  selector: 'dy-timeline-panel',
  imports: [Chart],
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
    <section class="busy" aria-label="Who was busiest">
      <div class="busy-head">
        <h2>Who was busiest</h2>
        <span class="at">{{ momentTime() }} · CPU {{ cpuAt() }}</span>
        <button type="button" class="btn sm ghost" (click)="peak.emit()">Jump to peak</button>
      </div>
      <table class="table">
        <caption class="visually-hidden">Top processes at {{ momentTime() }}</caption>
        <thead><tr><th>Process</th><th class="r">CPU, % of machine</th><th class="r">Memory</th></tr></thead>
        <tbody>
          @for (p of top(); track p.name) {
            <tr><td class="name" [title]="p.name">{{ p.name }}</td><td class="r">{{ p.cpu.toFixed(1) }}</td><td class="r">{{ bytes(p.memory) }}</td></tr>
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
    .plot:focus-visible { outline: 2px solid var(--focus); outline-offset: 2px; }
    .busy-head { display: flex; align-items: center; gap: var(--sp-3); margin-bottom: var(--sp-2); }
    .busy-head h2 { margin: 0; white-space: nowrap; font-size: var(--fs-md); font-weight: var(--fw-semibold); }
    .at { white-space: nowrap; color: var(--text-2); font-size: var(--fs-sm); font-variant-numeric: tabular-nums; }
    .busy-head .btn { margin-left: auto; }
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
  protected readonly option = computed(() => timelineOption(this.snapshot().samples, this.moment(), this.theme.tokens()));
  protected readonly summary = computed(() => timelineSummary(this.snapshot().samples));
  protected readonly colorA = computed(() => this.theme.tokens().chart[2]!);
  protected readonly colorB = computed(() => this.theme.tokens().chart[4]!);
  protected readonly headings = computed(() => panelHeadings(this.sample()));
  private readonly sample = computed(() => this.snapshot().samples[this.moment()]);
  protected readonly top = computed(() => this.sample()?.top ?? []);
  protected readonly momentTime = computed(() => clockSeconds(this.sample()?.at ?? this.snapshot().at));
  protected readonly cpuAt = computed(() => `${(this.sample()?.cpuTotal ?? 0).toFixed(1)}%`);

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
