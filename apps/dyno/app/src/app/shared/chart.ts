import { ChangeDetectionStrategy, Component, DestroyRef, ElementRef, afterNextRender, effect, inject, input, output, signal, viewChild } from '@angular/core';
import type { EChartsCoreOption, EChartsType } from 'echarts/core';

/** Whether the user asked the OS for less motion. */
export function prefersReducedMotion(): boolean {
  return typeof matchMedia === 'function' && matchMedia('(prefers-reduced-motion: reduce)').matches;
}

/**
 * Whether a chart animates this draw: only its first one, and never with reduced motion. Options are replaced whole
 * (`notMerge`), so an animated update replays the entry animation, about 18 canvas frames, on every poll of a live chart
 * (the Counters chart every 2 s); later draws therefore change in one frame.
 */
export function animateDraw(firstDraw: boolean, reducedMotion: boolean): boolean {
  return firstDraw && !reducedMotion;
}

/**
 * Whether options draw any data: a series or a dataset with at least one point. A skeleton or an empty first draw does not
 * count, so it does not use up the entry animation.
 */
export function hasData(options: EChartsCoreOption): boolean {
  const filled = (v: unknown): boolean => Array.isArray(v) ? v.length > 0 : typeof v === 'object' && v !== null && filled((v as { source?: unknown }).source);
  const list = (v: unknown): unknown[] => (Array.isArray(v) ? v : v ? [v] : []);
  return list(options['series']).some((s) => filled((s as { data?: unknown } | null)?.data)) || list(options['dataset']).some(filled);
}

/** Whether `doc` can give a canvas a 2D context, which ECharts' canvas renderer needs. */
function hasCanvas2d(doc: Document): boolean {
  try {
    return doc.createElement('canvas').getContext('2d') !== null;
  } catch {
    return false;
  }
}

/**
 * An Apache ECharts chart. ECharts loads on first use; the chart follows its container's size and re-renders when
 * `options` change (build them from `ThemeService.chart()` so both themes look right). A failure to render is
 * contained here and shown inline instead of breaking the page.
 */
@Component({
  selector: 'dy-chart',
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: { role: 'img', '[attr.aria-label]': 'label()' },
  template: `
    <div #host class="host" aria-hidden="true"></div>
    @if (failed()) {
      <p class="failed">This chart could not be drawn.</p>
    }
  `,
  styles: `
    :host { display: block; position: relative; height: var(--chart-h, 240px); min-width: 0; }
    .host { position: absolute; inset: 0; }
    .failed { position: absolute; inset: 0; display: grid; place-items: center; color: var(--text-3); font-size: var(--fs-sm); }
  `,
})
export class Chart {
  readonly options = input.required<EChartsCoreOption | null>();
  readonly label = input('');
  /** A click on a data item (`params` as ECharts gives them). */
  /** The user zoomed or panned the chart (wheel or drag). */
  readonly zoomed = output<void>();
  readonly itemClick = output<{ name?: string; data?: unknown; seriesName?: string }>();

  private readonly host = viewChild.required<ElementRef<HTMLDivElement>>('host');
  private chart: EChartsType | null = null;
  protected readonly failed = signal(false);
  private readonly ready = signal(false);
  /** Whether the chart has drawn data once (only that first draw animates). */
  private drawn = false;

  /**
   * Gives the chart the size its container has now, only when that differs from the chart's own. A container with no size
   * (the panel's skeleton is showing, so the projected chart is detached) is ignored: resizing to 0 left a 0 px canvas.
   */
  private fit(): void {
    const el = this.host().nativeElement;
    const chart = this.chart;
    if (!chart || el.clientWidth <= 0 || el.clientHeight <= 0) return;
    if (el.clientWidth !== chart.getWidth() || el.clientHeight !== chart.getHeight()) chart.resize({ width: el.clientWidth, height: el.clientHeight });
  }

  constructor() {
    const destroyRef = inject(DestroyRef);
    afterNextRender(() => {
      let observer: ResizeObserver | null = null;
      let disposed = false;
      destroyRef.onDestroy(() => {
        disposed = true;
        observer?.disconnect();
        // ECharts throws here when its canvas lost the 2D context after drawing; closing the page must not fail with it.
        try {
          this.chart?.dispose();
        } catch (e) {
          console.error('Chart failed to close', e);
        }
        this.chart = null;
      });
      import('./echarts-setup')
        .then(({ init }) => {
          if (disposed) return;
          // Without a 2D context (WebKit out of canvas memory, jsdom) ECharts throws in its frame loop and on dispose.
          if (!hasCanvas2d(this.host().nativeElement.ownerDocument)) {
            this.failed.set(true);
            return;
          }
          this.chart = init(this.host().nativeElement, null, { renderer: 'canvas' });
          this.chart.on('datazoom', () => this.zoomed.emit());
          this.chart.on('click', (p) => this.itemClick.emit(p as { name?: string; data?: unknown; seriesName?: string }));
          if (typeof ResizeObserver !== 'undefined') {
            observer = new ResizeObserver(() => this.fit());
            observer.observe(this.host().nativeElement);
          }
          this.ready.set(true);
        })
        .catch(() => this.failed.set(true));
    });

    effect(() => {
      const options = this.options();
      if (!this.ready() || !this.chart || !options) return;
      try {
        this.chart.setOption({ animationDuration: 300, ...options, animation: animateDraw(!this.drawn, prefersReducedMotion()) }, { notMerge: true, lazyUpdate: true });
        if (hasData(options)) this.drawn = true;
        this.failed.set(false);
        // The container may have been detached and back since the last resize (a panel's skeleton between two answers).
        requestAnimationFrame(() => this.fit());
      } catch (e) {
        console.error('Chart failed', e);
        this.failed.set(true);
      }
    });
  }
}
