import { ChangeDetectionStrategy, Component, computed, inject } from '@angular/core';
import { RouterLink } from '@angular/router';
import { formatGhz, formatPercent, formatCount, splitValue } from '../../shared/format';
import { Sparkline } from '../../shared/sparkline';
import { LiveStore } from '../../state/live.store';
import { Icon } from '../../shared/icon';
import { ViewPrefs } from '../../core/view-prefs';
import { coreGroups } from '../cpu/core-layout';
import { MatIconButton } from '@angular/material/button';
import { SuSegmentedControl, type SuSegment } from '@decryp7/paddock-ui';
import { MatTooltip } from '@angular/material/tooltip';

/** The number and unit of a formatted value, for the hero (`<b>` number, `<small>` unit). */
const parts = splitValue;
const CPU_VIEWS: SuSegment[] = [{ value: 'overall', label: 'Overall' }, { value: 'cores', label: 'Per core' }];

/** CPU: hero, one sparkline, caption rows; the toggle switches to one live bar per core grouped by core kind. */
@Component({
  selector: 'dy-cpu-tile',
  imports: [MatTooltip, SuSegmentedControl, MatIconButton, Sparkline, RouterLink, Icon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @if (cpu(); as c) {
      <div class="head">
        <div class="hero"><b>{{ total(c.total)[0] }}</b><small>{{ total(c.total)[1] }}</small></div>
        <span class="spacer"></span>
        <pd-segmented-control label="CPU view" [options]="cpuViews" [value]="prefs.cpuView()" (valueChange)="prefs.setCpuView($event === 'cores' ? 'cores' : 'overall')" />
        <a matIconButton routerLink="/cpu" aria-label="CPU details" matTooltipPosition="left" [matTooltip]="'CPU details'"><dy-icon name="arrow-right" /></a>
      </div>
      @if (prefs.cpuView() === 'overall') {
        <dy-sparkline [values]="live.series().cpu" [max]="100" color="var(--accent)" label="CPU use over the last minute" style="--spark-h: 32px" />
        <div class="cores" role="img" [attr.aria-label]="coreLabel()">
          @for (v of c.perCore; track $index) {
            <span class="core" [style.background]="coreColor(v)" [matTooltip]="'Core ' + count($index) + ': ' + pct(v)"></span>
          }
        </div>
        <dl class="caps">
          <div><dt>Frequency</dt><dd>{{ ghz(c.frequencyMhz) }} · base {{ ghz(c.baseFrequencyMhz) }}</dd></div>
          <div><dt>Cores</dt><dd>{{ coresText() }}</dd></div>
        </dl>
      } @else {
        <div class="groups">
          @for (g of groups(); track g.kind) {
            <section [attr.aria-label]="g.title">
              <h3>{{ g.title }}</h3>
              <ul>
                @for (core of g.cores; track core.index) {
                  <li>
                    <span class="lbl num">{{ core.label }}</span>
                    <span class="bar-track" role="img" [attr.aria-label]="core.label + ' ' + pct(c.perCore[core.index] ?? 0)"><span [style.width.%]="c.perCore[core.index] ?? 0"></span></span>
                    <span class="val num">{{ pct(c.perCore[core.index] ?? 0) }}</span>
                  </li>
                }
              </ul>
            </section>
          }
        </div>
      }
    }
  `,
  styles: `
    :host { display: grid; gap: var(--sp-3); min-width: 0; align-content: start; }
    .head { display: flex; align-items: center; gap: var(--sp-1); flex-wrap: wrap; }
    .spacer { flex: 1; }
    .cores { display: grid; grid-auto-flow: column; grid-auto-columns: 1fr; gap: 2px; height: 12px; }
    .core { border-radius: 2px; background-color: var(--surface-active); }
    .groups { display: grid; gap: var(--sp-3); }
    h3 { font-size: var(--fs-xs); color: var(--text-3); font-weight: var(--fw-semibold); margin-bottom: var(--sp-1); }
    ul { list-style: none; margin: 0; padding: 0; display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 3px var(--sp-4); }
    li { display: grid; grid-template-columns: 22px minmax(0, 1fr) 38px; align-items: center; gap: var(--sp-2); font-size: var(--fs-xs); }
    .lbl { color: var(--text-3); }
    .val { text-align: right; }
    .bar-track { height: 4px; }
  `,
})
export class CpuTile {
  protected readonly live = inject(LiveStore);
  protected readonly prefs = inject(ViewPrefs);
  protected readonly cpuViews = CPU_VIEWS;
  protected readonly cpu = computed(() => this.live.system()?.cpu ?? null);
  protected readonly groups = computed(() => {
    const c = this.cpu();
    return c ? coreGroups(c.coreKinds, c.perCore.length) : [];
  });
  protected readonly pct = formatPercent;
  protected readonly ghz = formatGhz;
  protected readonly total = (v: number) => parts(formatPercent(v));
  protected readonly count = (i: number) => formatCount(i + 1);
  /** A single-hue ramp from neutral to the accent (percent load; the number is in the tooltip). */
  protected readonly coreColor = (v: number) => `color-mix(in srgb, var(--accent) ${Math.round(Math.min(100, Math.max(0, v)) * 0.85)}%, var(--surface-active))`;
  protected readonly coresText = computed(() => {
    const c = this.cpu();
    if (!c) return '';
    return `${formatCount(c.perCore.length)} logical · busiest ${formatPercent(Math.max(0, ...c.perCore))}`;
  });
  protected readonly coreLabel = computed(() => `Per-core use: ${this.cpu()?.perCore.map((v) => formatPercent(v)).join(', ') ?? ''}`);
}
