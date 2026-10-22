import { ChangeDetectionStrategy, Component, ElementRef, afterNextRender, effect, signal, viewChild } from '@angular/core';
import * as echarts from 'echarts/core';
import { LineChart, ScatterChart } from 'echarts/charts';
import { GridComponent, LegendComponent, MarkLineComponent, TooltipComponent } from 'echarts/components';
import { CanvasRenderer } from 'echarts/renderers';
import {
  SU_ACCENTS,
  SU_DENSITIES,
  SU_THEMES,
  SuCard,
  SuEmptyState,
  SuErrorState,
  SuMetric,
  SuSegmentedControl,
  SuSkeleton,
  SuSparkline,
  SuStatusPill,
  SuTrayPopover,
  applySuiteAppearance,
  areaGradient,
  eventLaneFragment,
  readSuiteTokens,
  registerSuiteTheme,
  type SuAccent,
  type SuDensity,
  type SuTheme,
} from '@decryp7/suite-ui';

echarts.use([LineChart, ScatterChart, GridComponent, LegendComponent, TooltipComponent, MarkLineComponent, CanvasRenderer]);

function param<T extends string>(name: string, allowed: readonly T[], fallback: T): T {
  const v = new URLSearchParams(location.search).get(name);
  return allowed.find((a) => a === v) ?? fallback;
}

const CPU = [22, 25, 31, 28, 40, 52, 47, 38, 35, 44, 61, 58, 49, 42, 39, 45];
const MEM = [60, 61, 61, 62, 64, 66, 66, 67, 69, 70, 70, 71, 71, 72, 72, 72];
const NET = [4, 6, 5, 9, 22, 18, 12, 9, 7, 8, 30, 26, 14, 10, 8, 9];

@Component({
  selector: 'gal-root',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [SuCard, SuStatusPill, SuMetric, SuSparkline, SuEmptyState, SuErrorState, SuSkeleton, SuSegmentedControl, SuTrayPopover],
  template: `
    <main class="gal-page">
      <header class="gal-bar">
        <h1>suite-ui gallery</h1>
        <su-segmented-control label="Theme" [options]="themes" [(value)]="theme" />
        <su-segmented-control label="Accent" [options]="accents" [(value)]="accent" />
        <su-segmented-control label="Density" [options]="densities" [(value)]="density" />
      </header>

      <section class="gal-section" data-testid="card" aria-labelledby="h-card">
        <h2 id="h-card">Card / tile</h2>
        <div class="gal-grid">
          <su-card heading="CPU">
            <svg slot="icon" viewBox="0 0 16 16" width="16" height="16" aria-hidden="true"><rect x="4" y="4" width="8" height="8" rx="1.5" fill="none" stroke="currentColor" stroke-width="1.5" /></svg>
            <su-status-pill slot="status" status="success" label="Normal" />
            <su-metric value="45" unit="%" [delta]="3" deltaUnit=" pts" deltaLabel="vs last hour" goodDirection="down" />
            <su-sparkline slot="sparkline" label="CPU, last hour" [values]="cpu" [area]="true" [min]="0" [max]="100" />
            <span slot="caption">peak 61% at 14:20</span>
          </su-card>
          <su-card heading="Memory">
            <su-status-pill slot="status" status="warning" label="High" />
            <su-metric [value]="17.1" unit="GB" [decimals]="1" />
            <su-sparkline slot="sparkline" label="Memory, last hour" [values]="mem" [min]="0" [max]="100" />
            <span slot="caption">72% of 24 GB</span>
          </su-card>
          <su-card heading="Network" [interactive]="true" (activated)="clicks.set(clicks() + 1)">
            <su-metric [value]="5.02" unit="MB/s" [decimals]="2" />
            <su-sparkline slot="sparkline" label="Down, last hour" [values]="net" />
            <span slot="caption">Up 0.4 MB/s · activated {{ clicks() }}×</span>
          </su-card>
          <su-card heading="Runners">
            <su-status-pill slot="status" status="danger" label="1 failing" />
            <su-metric [value]="12408" size="display" />
            <span slot="caption">jobs this week</span>
          </su-card>
        </div>
      </section>

      <section class="gal-section" data-testid="status-pill" aria-labelledby="h-pill">
        <h2 id="h-pill">Status pill</h2>
        <div class="gal-row">
          <su-status-pill status="success" label="Healthy" />
          <su-status-pill status="warning" label="Needs attention" />
          <su-status-pill status="danger" label="Failing" />
          <su-status-pill status="info" label="Update available" />
          <su-status-pill status="neutral" label="Paused" />
        </div>
      </section>

      <section class="gal-section" data-testid="metric" aria-labelledby="h-metric">
        <h2 id="h-metric">Metric</h2>
        <div class="gal-row">
          <su-metric [value]="74" unit="/100" />
          <su-metric [value]="1234567" />
          <su-metric [value]="3.2" unit="%" size="title" [delta]="-1.4" deltaUnit="%" deltaLabel="vs last week" goodDirection="down" />
          <su-metric [value]="86" unit="%" size="title" [delta]="12" deltaUnit="%" goodDirection="down" />
          <su-metric [value]="18.4" unit="ms" size="body" [decimals]="1" [delta]="0" />
          <su-metric [value]="null" />
        </div>
      </section>

      <section class="gal-section" data-testid="sparkline" aria-labelledby="h-spark">
        <h2 id="h-spark">Sparkline</h2>
        <div class="gal-row">
          <div class="gal-box gal-skel-card"><su-sparkline label="CPU" [values]="cpu" [area]="true" /></div>
          <div class="gal-box gal-skel-card"><su-sparkline label="Memory" [values]="mem" [height]="24" /></div>
          <div class="gal-box gal-skel-card"><su-sparkline label="Gap" [values]="gap" /></div>
        </div>
      </section>

      <section class="gal-section" data-testid="empty-state" aria-labelledby="h-empty">
        <h2 id="h-empty">Empty state</h2>
        <div class="gal-box">
          <su-empty-state heading="No runners yet" description="Register a runner and its jobs appear here with a live timer.">
            <button slot="action" type="button" class="gal-btn gal-btn--primary">Register runner</button>
          </su-empty-state>
        </div>
      </section>

      <section class="gal-section" data-testid="error-state" aria-labelledby="h-error">
        <h2 id="h-error">Error state</h2>
        <div class="gal-box">
          <su-error-state heading="Can't reach the Gitea server" cause="Check the URL in Settings, then retry." secondaryLabel="Open logs" (retry)="retries.set(retries() + 1)" />
          <p class="gal-num gal-row">Retried {{ retries() }}×</p>
        </div>
      </section>

      <section class="gal-section" data-testid="skeleton" aria-labelledby="h-skel">
        <h2 id="h-skel">Skeleton</h2>
        <div class="gal-row">
          <div class="gal-box gal-skel-card">
            <su-skeleton width="40%" />
            <su-skeleton shape="block" height="28" width="60%" />
            <su-skeleton shape="block" height="32" />
            <su-skeleton width="80%" />
          </div>
          <su-skeleton shape="circle" width="40" height="40" />
        </div>
      </section>

      <section class="gal-section" data-testid="segmented-control" aria-labelledby="h-seg">
        <h2 id="h-seg">Segmented control</h2>
        <div class="gal-row">
          <su-segmented-control label="Time range" [options]="ranges" [(value)]="range" />
          <su-segmented-control label="Level" [options]="levels" [(value)]="level" />
          <span class="gal-num">Selected: {{ range() }} / {{ level() }}</span>
        </div>
      </section>

      <section class="gal-section" data-testid="tray-popover" aria-labelledby="h-tray">
        <h2 id="h-tray">Tray popover</h2>
        <div class="gal-tray-stage">
          <su-tray-popover label="Dyno" platform="macos">
            <div slot="header" class="gal-inline">
              <strong>Dyno</strong>
              <su-status-pill status="success" label="Good" />
            </div>
            <div class="gal-tray-tiles">
              <su-card heading="CPU" [flat]="true"><su-metric [value]="45" unit="%" size="title" /><su-sparkline slot="sparkline" label="CPU" [values]="cpu" [height]="24" /></su-card>
              <su-card heading="Memory" [flat]="true"><su-metric [value]="72" unit="%" size="title" /><su-sparkline slot="sparkline" label="Memory" [values]="mem" [height]="24" /></su-card>
            </div>
            <ul class="gal-list">
              <li><span class="grow">rustc</span><span class="gal-num">42%</span></li>
              <li><span class="grow">WindowServer</span><span class="gal-num">11%</span></li>
              <li><span class="grow">Safari</span><span class="gal-num">6%</span></li>
            </ul>
            <div slot="footer" class="gal-inline">
              <button type="button" class="gal-btn gal-btn--primary">Open Dyno</button>
              <button type="button" class="gal-btn gal-btn--icon" aria-label="Pause monitoring">II</button>
            </div>
          </su-tray-popover>
        </div>
      </section>

      <section class="gal-section" data-testid="chart-theme" aria-labelledby="h-chart">
        <h2 id="h-chart">ECharts theme</h2>
        <div class="gal-box"><div #chart class="gal-chart" role="img" aria-label="CPU and memory, last 16 samples, with events"></div></div>
        <div class="gal-swatches">
          @for (n of [1, 2, 3, 4, 5, 6, 7, 8]; track n) { <span class="gal-swatch gal-swatch--{{ n }}"></span> }
        </div>
        <div class="gal-swatches">
          @for (n of ['s1', 's2', 's3', 's4', 's5', 's6']; track n) { <span class="gal-swatch gal-swatch--{{ n }}"></span> }
        </div>
      </section>
    </main>
  `,
})
export class Gallery {
  protected readonly cpu = CPU;
  protected readonly mem = MEM;
  protected readonly net = NET;
  protected readonly gap = [10, 14, 12, Number.NaN, Number.NaN, 30, 28, 33, 29];
  protected readonly themes = SU_THEMES.map((value) => ({ value, label: value }));
  protected readonly accents = SU_ACCENTS.map((value) => ({ value, label: value }));
  protected readonly densities = SU_DENSITIES.map((value) => ({ value, label: value }));
  protected readonly ranges = [
    { value: '1h', label: '1 h' },
    { value: '24h', label: '24 h' },
    { value: '7d', label: '7 days' },
  ];
  protected readonly levels = [
    { value: 'all', label: 'All', count: 1204 },
    { value: 'warn', label: 'Warnings', count: 31 },
    { value: 'err', label: 'Errors', count: 6 },
    { value: 'off', label: 'Off', disabled: true },
  ];

  protected readonly theme = signal<string>(param<SuTheme>('theme', SU_THEMES, 'system'));
  protected readonly accent = signal<string>(param<SuAccent>('accent', SU_ACCENTS, 'debrief'));
  protected readonly density = signal<string>(param<SuDensity>('density', SU_DENSITIES, 'standard'));
  protected readonly range = signal('24h');
  protected readonly level = signal('all');
  protected readonly clicks = signal(0);
  protected readonly retries = signal(0);

  private readonly chartEl = viewChild.required<ElementRef<HTMLElement>>('chart');
  private chart: echarts.ECharts | null = null;

  constructor() {
    effect(() => {
      applySuiteAppearance(document.documentElement, {
        theme: this.theme() as SuTheme,
        accent: this.accent() as SuAccent,
        density: this.density() as SuDensity,
      });
      // Appearance changed: draw the chart again with re-read tokens.
      queueMicrotask(() => this.draw());
    });
    afterNextRender(() => this.draw());
  }

  private draw(): void {
    const el = this.chartEl().nativeElement;
    const tokens = readSuiteTokens();
    const name = registerSuiteTheme(echarts, tokens);
    this.chart?.dispose();
    const chart = echarts.init(el, name, { renderer: 'canvas' });
    this.chart = chart;
    const t0 = Date.UTC(2026, 9, 22, 8, 0, 0);
    const times = CPU.map((_, i) => t0 + i * 15 * 60_000);
    const lane = eventLaneFragment(
      [
        { at: times[3] as number, kind: 'throttle', label: 'Throttled' },
        { at: times[6] as number, kind: 'cap', label: 'Memory cap' },
        { at: times[10] as number, kind: 'throttle', label: 'Throttled' },
        { at: times[13] as number, kind: 'release', label: 'Released' },
      ],
      tokens,
      1,
      { shapes: { throttle: 'triangle', cap: 'rect', release: 'circle' } },
    );
    chart.setOption({
      grid: [{ left: 8, right: 16, top: 32, bottom: lane.plotBottom, containLabel: true }, lane.grid],
      legend: { data: ['CPU', 'Memory'] },
      xAxis: [{ type: 'time' }, lane.xAxis],
      yAxis: [{ type: 'value', min: 0, max: 100, interval: 50, axisLabel: { formatter: '{value}%' } }, lane.yAxis],
      series: [
        {
          name: 'CPU',
          type: 'line',
          data: CPU.map((v, i) => [times[i], v]),
          areaStyle: { color: areaGradient(tokens.palette[0] as string) },
          lineStyle: { width: 2 },
          markLine: { silent: true, data: [{ yAxis: 90, label: { formatter: 'limit 90%' } }] },
        },
        { name: 'Memory', type: 'line', data: MEM.map((v, i) => [times[i], v]) },
        lane.series,
      ],
    });
  }
}
