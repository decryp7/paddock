import {
  ChangeDetectionStrategy,
  Component,
  DestroyRef,
  ElementRef,
  InjectionToken,
  afterNextRender,
  effect,
  inject,
  input,
  signal,
  viewChild,
} from '@angular/core';
import type { EChartsCoreOption } from 'echarts/core';

/** The part of an ECharts instance `pc-chart` uses. */
export interface ChartInstance {
  setOption(option: EChartsCoreOption, opts?: { notMerge?: boolean; lazyUpdate?: boolean }): void;
  resize(): void;
  dispose(): void;
}

/** Creates a chart in an element (ECharts' `init`, lazily loaded; a fake in tests). */
export type ChartFactory = (el: HTMLElement) => ChartInstance;

/** Loads the chart factory. The default imports the tree-shaken ECharts build on first use. */
export const CHART_LOADER = new InjectionToken<() => Promise<ChartFactory>>('ChartLoader', {
  providedIn: 'root',
  factory: () => () => import('./echarts-setup').then((m) => (el: HTMLElement) => m.init(el, null, { renderer: 'svg' }) as unknown as ChartInstance),
});

/**
 * An ECharts chart. The option is built by a pure function from data and the theme's chart tokens; a new option
 * (new data or a theme change) replaces the old one. The chart has a text alternative (`label`).
 */
@Component({
  selector: 'pc-chart',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div #host class="host" role="img" [attr.aria-label]="label()" [style.height.px]="height()"></div>
    @if (failed()) {
      <p class="failed">The chart could not be drawn.</p>
    }
  `,
  styles: `
    :host {
      display: block;
      position: relative;
      min-width: 0;
    }
    .host {
      width: 100%;
    }
    .failed {
      position: absolute;
      inset: 0;
      display: grid;
      place-items: center;
      color: var(--text-3);
      font-size: var(--text-sm);
    }
  `,
})
export class Chart {
  readonly option = input.required<EChartsCoreOption>();
  readonly label = input.required<string>();
  readonly height = input(220);

  private readonly host = viewChild.required<ElementRef<HTMLElement>>('host');
  private readonly loader = inject(CHART_LOADER);
  private readonly instance = signal<ChartInstance | null>(null);
  protected readonly failed = signal(false);
  /** Charts draw without animation when the user asked for reduced motion. */
  private readonly reducedMotion = globalThis.matchMedia?.('(prefers-reduced-motion: reduce)').matches ?? false;

  constructor() {
    const destroyRef = inject(DestroyRef);
    let observer: ResizeObserver | null = null;
    let destroyed = false;

    afterNextRender(() => {
      this.loader().then(
        (factory) => {
          if (destroyed) return;
          try {
            const chart = factory(this.host().nativeElement);
            this.instance.set(chart);
            if (typeof ResizeObserver !== 'undefined') {
              observer = new ResizeObserver(() => chart.resize());
              observer.observe(this.host().nativeElement);
            }
          } catch {
            this.failed.set(true);
          }
        },
        () => this.failed.set(true),
      );
    });

    effect(() => {
      const chart = this.instance();
      const option = this.option();
      if (chart) {
        try {
          chart.setOption(this.reducedMotion ? { ...option, animation: false } : option, { notMerge: true });
        } catch {
          this.failed.set(true);
        }
      }
    });

    destroyRef.onDestroy(() => {
      destroyed = true;
      observer?.disconnect();
      this.instance()?.dispose();
    });
  }
}
