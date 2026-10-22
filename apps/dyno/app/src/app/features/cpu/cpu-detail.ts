import { ChangeDetectionStrategy, Component, OnInit, computed, inject } from '@angular/core';
import { RouterLink } from '@angular/router';
import { ThemeService } from '../../core/theme';
import { AsyncState } from '../../shared/async-state';
import { Chart } from '../../shared/chart';
import { formatGhz, formatPercent } from '../../shared/format';
import { Icon } from '../../shared/icon';
import { EVENT_LABEL } from '../../shared/labels';
import { HistoryStore } from '../../state/history.store';
import { LiveStore } from '../../state/live.store';
import { RANGES } from '../dashboard/history-panels';
import { coreGroups } from './core-layout';
import { coreHeatmapOption } from './cpu-options';

/** The CPU in detail: every logical core now (load and frequency, grouped by kind), the hottest apps, and a cores x time heatmap. */
@Component({
  selector: 'dy-cpu-detail',
  imports: [RouterLink, Icon, Chart, AsyncState],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="page">
      <header class="page-head">
        <a class="btn ghost icon" routerLink="/" aria-label="Back to the dashboard"><dy-icon name="chevron-left" /></a>
        <div class="titles">
          <h1>CPU</h1>
          <p class="num">{{ summary() }}</p>
        </div>
      </header>

      <div class="grid">
        <section class="card now" aria-label="Cores now">
          <div class="card-head"><h2><dy-icon name="cpu" /> Cores right now</h2></div>
          @if (cpu(); as c) {
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
                        <span class="freq num">{{ freq(core.index) }}</span>
                      </li>
                    }
                  </ul>
                </section>
              }
            </div>
          } @else {
            <div class="skeletons"><div class="skeleton"></div><div class="skeleton"></div><div class="skeleton"></div></div>
          }
        </section>

        <section class="card apps" aria-label="Hottest apps">
          <div class="card-head"><h2><dy-icon name="processes" /> Hottest apps</h2></div>
          <ol>
            @for (g of hot(); track g.id) {
              <li><span class="truncate">{{ g.name }}</span><span class="num">{{ pct(g.cpu) }}</span></li>
            } @empty {
              <li class="faint">Waiting for the first reading…</li>
            }
          </ol>
        </section>

        <section class="card map" aria-label="Per-core history">
          <div class="card-head">
            <h2><dy-icon name="trend" /> Load per core over time</h2>
            <span class="spacer"></span>
            <div class="seg" role="group" aria-label="Range">
              @for (r of ranges; track r.id) {
                <button type="button" [attr.aria-pressed]="history.range() === r.id" (click)="history.setRange(r.id)">{{ r.label }}</button>
              }
            </div>
          </div>
          <dy-async [loaded]="!!history.history()" [error]="history.error()" [empty]="noCores()" emptyTitle="No per-core history yet" emptyText="The agent records every core's load; the heatmap fills in as time passes." what="per-core history" [rows]="0" (retry)="history.load()">
            <div skeleton class="skeleton chart-skeleton"></div>
            <dy-chart [options]="option()" label="Heatmap of each core's load over time" [style.--chart-h]="chartHeight() + 'px'" />
            <p class="legend">Dashed lines mark events:
              @for (e of eventKinds(); track e) {<span class="ev">{{ e }}</span>}
            </p>
          </dy-async>
        </section>
      </div>
    </div>
  `,
  styles: `
    :host { display: block; }
    .page-head { align-items: center; }
    .grid { display: grid; grid-template-columns: minmax(0, 2fr) minmax(0, 1fr); gap: var(--sp-4); }
    .map { grid-column: 1 / -1; padding-bottom: var(--sp-3); }
    @media (max-width: 900px) { .grid { grid-template-columns: 1fr; } }
    .now, .apps { padding-bottom: var(--sp-4); }
    .groups { display: grid; gap: var(--sp-4); padding: var(--sp-3) var(--sp-4) 0; }
    h3 { font-size: var(--fs-xs); color: var(--text-3); font-weight: var(--fw-semibold); letter-spacing: 0.04em; text-transform: uppercase; margin-bottom: var(--sp-2); }
    ul { list-style: none; margin: 0; padding: 0; display: grid; grid-template-columns: repeat(auto-fill, minmax(260px, 1fr)); gap: 6px var(--sp-5); }
    li { display: grid; grid-template-columns: 26px minmax(0, 1fr) 44px 70px; align-items: center; gap: var(--sp-2); font-size: var(--fs-sm); }
    .lbl { color: var(--text-3); }
    .val { text-align: right; font-weight: var(--fw-medium); }
    .freq { text-align: right; color: var(--text-3); font-size: var(--fs-xs); }
    .apps ol { list-style: none; margin: 0; padding: var(--sp-2) var(--sp-4) 0; display: grid; }
    .apps li { grid-template-columns: minmax(0, 1fr) auto; padding: 7px 0; border-bottom: 1px solid var(--border-subtle); }
    .faint { color: var(--text-3); }
    .skeletons { display: grid; gap: var(--sp-2); padding: var(--sp-4); } .skeleton { height: 16px; }
    dy-async { padding: 0 var(--sp-4); }
    .chart-skeleton { height: 360px; margin-top: var(--sp-2); }
    .legend { color: var(--text-3); font-size: var(--fs-xs); display: flex; gap: var(--sp-3); flex-wrap: wrap; padding-top: var(--sp-1); }
    .ev { padding: 0 var(--sp-2); border: 1px solid var(--border); border-radius: var(--r-pill); }
  `,
})
export class CpuDetail implements OnInit {
  protected readonly live = inject(LiveStore);
  protected readonly history = inject(HistoryStore);
  private readonly theme = inject(ThemeService);
  protected readonly ranges = RANGES;
  protected readonly pct = formatPercent;
  protected readonly cpu = computed(() => this.live.system()?.cpu ?? null);
  protected readonly groups = computed(() => {
    const c = this.cpu();
    return c ? coreGroups(c.coreKinds, c.perCore.length) : [];
  });
  protected readonly hot = computed(() => (this.live.update()?.topCpu ?? []).slice(0, 8));
  protected readonly noCores = computed(() => (this.history.history()?.corePoints.length ?? 0) === 0);
  protected readonly option = computed(() => {
    const h = this.history.history();
    const c = this.cpu();
    return h && h.corePoints.length ? coreHeatmapOption(h, c?.coreKinds ?? [], this.theme.tokens()) : null;
  });
  protected readonly chartHeight = computed(() => Math.max(240, 96 + 22 * (this.cpu()?.perCore.length ?? 8)));
  protected readonly eventKinds = computed(() => [...new Set((this.history.history()?.events ?? []).map((e) => EVENT_LABEL[e.kind]))]);
  protected readonly summary = computed(() => {
    const c = this.cpu();
    return c ? `${pctText(c.total)} overall · ${formatGhz(c.frequencyMhz)} · ${c.perCore.length === 1 ? '1 logical core' : `${c.perCore.length.toString()} logical cores`}` : 'Reading the CPU…';
  });

  protected freq(i: number): string {
    const f = this.cpu()?.coreFrequencyMhz[i];
    return f ? formatGhz(f) : '';
  }

  ngOnInit(): void {
    void this.history.ensure();
  }
}

const pctText = formatPercent;
