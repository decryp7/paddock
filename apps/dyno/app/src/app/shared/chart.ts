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
  /** Whether the chart has drawn once (only the first draw animates). */
  private drawn = false;

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
            observer = new ResizeObserver(() => this.chart?.resize());
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
        this.chart.setOption({ animation: animateDraw(!this.drawn, prefersReducedMotion()), animationDuration: 300, ...options }, { notMerge: true, lazyUpdate: true });
        this.drawn = true;
        this.failed.set(false);
      } catch (e) {
        console.error('Chart failed', e);
        this.failed.set(true);
      }
    });
  }
}
