import { ChangeDetectionStrategy, Component, DestroyRef, ElementRef, InjectionToken, afterNextRender, effect, inject, input, output, untracked } from '@angular/core';
import type { EChartsCoreOption } from 'echarts/core';

/** The slice of an ECharts instance Pitwall uses. */
export interface ChartInstance {
  setOption(option: EChartsCoreOption, opts?: { notMerge?: boolean; lazyUpdate?: boolean; replaceMerge?: string[] }): void;
  resize(): void;
  dispose(): void;
  on(event: string, handler: (params: unknown) => void): void;
}

export type ChartFactory = (el: HTMLElement) => Promise<ChartInstance>;

/** Creates chart instances; lazy-loads ECharts so it never weighs on the first paint. Tests provide a fake. */
export const CHART_FACTORY = new InjectionToken<ChartFactory>('ChartFactory', {
  providedIn: 'root',
  factory:
    /* v8 ignore next 2 -- the real factory needs a canvas */
    () => (el) => import('./echarts-setup').then(({ echarts }) => echarts.init(el, null, { renderer: 'canvas' }) as unknown as ChartInstance),
});

/**
 * One ECharts chart. Updates are coalesced to one `setOption` per animation frame and merged (`notMerge: false`,
 * `lazyUpdate`), so pushing points every second stays cheap; a changed `themeKey` replaces the whole option so every
 * colour follows the theme.
 */
@Component({
  selector: 'pw-echart',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: ``,
  host: { role: 'img', '[attr.aria-label]': 'label()', '[style.height]': 'height()' },
  styles: `
    :host {
      display: block;
      width: 100%;
      min-width: 0;
    }
  `,
})
export class EChart {
  readonly option = input.required<EChartsCoreOption>();
  /** Accessible summary of what the chart shows (the data is also available as text nearby). */
  readonly label = input.required<string>();
  readonly height = input('220px');
  /** Changes when the theme does: the next update replaces instead of merging. */
  readonly themeKey = input<string>('');
  readonly chartEvent = output<{ type: string; params: unknown }>();

  private readonly el = inject(ElementRef<HTMLElement>).nativeElement as HTMLElement;
  private readonly factory = inject(CHART_FACTORY);
  private chart: ChartInstance | null = null;
  private pending: EChartsCoreOption | null = null;
  private replace = true;
  private frame: number | null = null;
  private lastTheme = '';
  private destroyed = false;

  constructor() {
    effect(() => {
      const option = this.option();
      const theme = this.themeKey();
      untracked(() => this.queue(option, theme));
    });
    afterNextRender(() => {
      void this.factory(this.el).then((chart) => {
        if (this.destroyed) {
          chart.dispose();
          return;
        }
        this.chart = chart;
        for (const type of ['datazoom', 'click']) chart.on(type, (params) => this.chartEvent.emit({ type, params }));
        this.flush();
      });
    });
    const ro = new ResizeObserver(() => this.chart?.resize());
    ro.observe(this.el);
    inject(DestroyRef).onDestroy(() => {
      this.destroyed = true;
      ro.disconnect();
      if (this.frame !== null) cancelAnimationFrame(this.frame);
      this.chart?.dispose();
    });
  }

  private queue(option: EChartsCoreOption, theme: string): void {
    if (theme !== this.lastTheme) {
      this.replace = true;
      this.lastTheme = theme;
    }
    this.pending = option;
    if (this.chart && this.frame === null) this.frame = requestAnimationFrame(() => this.flush());
  }

  private flush(): void {
    this.frame = null;
    if (!this.chart || !this.pending) return;
    const replace = this.replace;
    this.chart.setOption(this.pending, { notMerge: replace, lazyUpdate: !replace });
    this.replace = false;
    this.pending = null;
  }
}
