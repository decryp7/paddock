import { ChangeDetectionStrategy, Component, ElementRef, InjectionToken, OnDestroy, effect, inject, input, viewChild } from '@angular/core';
import { registerPaddockTheme, type PaddockChartTokens } from '@decryp7/paddock-ui';
import { ThemeService } from '../../core/theme.service';

/** The part of an ECharts instance the app uses. */
export interface ChartInstance {
  setOption(option: unknown, notMerge?: boolean): void;
  resize(): void;
  dispose(): void;
}

export type ChartFactory = (el: HTMLElement) => ChartInstance;

/** Loads ECharts (only the parts used) when the first chart is shown, and registers the paddock theme for the tokens. */
export type ChartLoader = (tokens: PaddockChartTokens) => Promise<ChartFactory>;

async function loadEcharts(tokens: PaddockChartTokens): Promise<ChartFactory> {
  const [core, charts, components, renderers] = await Promise.all([
    import('echarts/core'),
    import('echarts/charts'),
    import('echarts/components'),
    import('echarts/renderers'),
  ]);
  core.use([charts.BarChart, charts.LineChart, components.GridComponent, components.TooltipComponent, components.LegendComponent, components.AriaComponent, renderers.CanvasRenderer]);
  const theme = registerPaddockTheme(core, tokens);
  return (el) => core.init(el, theme) as unknown as ChartInstance;
}

export const CHART_LOADER = new InjectionToken<ChartLoader>('CHART_LOADER', { providedIn: 'root', factory: () => loadEcharts });

/** A themed chart. It is redrawn when the option or the colour scheme changes and resized with its container. */
@Component({
  selector: 'sp-chart',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `<div #host class="host" role="img" [attr.aria-label]="label()"></div>`,
  styles: `
    :host {
      display: block;
      height: 100%;
    }
    .host {
      width: 100%;
      height: 100%;
      min-height: 120px;
    }
  `,
})
export class Chart implements OnDestroy {
  /** An ECharts option object built by the pure builders in `chart-options.ts`. */
  readonly option = input.required<unknown>();
  /** What the chart shows, in words, for assistive technology. */
  readonly label = input.required<string>();

  private readonly theme = inject(ThemeService);
  private readonly loader = inject(CHART_LOADER);
  private readonly host = viewChild.required<ElementRef<HTMLElement>>('host');
  private chart: ChartInstance | null = null;
  private factoryFor: PaddockChartTokens | null = null;
  private observer: ResizeObserver | null = null;
  private run = 0;
  private destroyed = false;

  constructor() {
    effect(() => {
      const option = this.option();
      const tokens = this.theme.chartTokens();
      void this.draw(option, tokens);
    });
  }

  private async draw(option: unknown, tokens: PaddockChartTokens): Promise<void> {
    const run = ++this.run;
    if (this.chart && this.factoryFor !== tokens) {
      this.chart.dispose();
      this.chart = null;
    }
    if (!this.chart) {
      let factory: ChartFactory;
      try {
        factory = await this.loader(tokens);
      } catch {
        return; // The chart stays empty; the data is also shown as text elsewhere on the page.
      }
      if (this.destroyed || run !== this.run) return;
      this.chart = factory(this.host().nativeElement);
      this.factoryFor = tokens;
      if (typeof ResizeObserver !== 'undefined') {
        this.observer?.disconnect();
        this.observer = new ResizeObserver(() => this.chart?.resize());
        this.observer.observe(this.host().nativeElement);
      }
    }
    this.chart.setOption(option, true);
  }

  ngOnDestroy(): void {
    this.destroyed = true;
    this.observer?.disconnect();
    this.chart?.dispose();
    this.chart = null;
  }
}
