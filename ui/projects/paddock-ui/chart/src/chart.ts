import {
  afterNextRender,
  ChangeDetectionStrategy,
  Component,
  computed,
  DestroyRef,
  effect,
  ElementRef,
  inject,
  InjectionToken,
  input,
  output,
  signal,
  viewChild,
} from '@angular/core';
import { readPaddockTokens, registerPaddockTheme, type PaddockChartTokens } from '@decryp7/paddock-ui';
import { PdGlyph } from '@decryp7/paddock-ui/icons';
import { chartOption, chartSummary, donutNarrowHeight, type PdChartSpec } from './options';

/** The part of an ECharts instance `pd-chart` calls (`FakeChart` of the test kit has the same shape). */
export interface PdChartInstance {
  setOption(option: unknown, opts?: unknown): void;
  resize(): void;
  dispose(): void;
  on(name: string, handler: (event: unknown) => void): void;
}

/** What the loader gives `pd-chart`: ECharts' `init` and `registerTheme` (or fakes). */
export interface PdEchartsLib {
  init(element: HTMLElement, theme?: string, opts?: unknown): PdChartInstance;
  registerTheme(name: string, theme: object): void;
}

/**
 * How `pd-chart` gets ECharts. The default imports `echarts` on first use (a lazy chunk, so the initial bundle stays small);
 * an app that wants a tree-shaken build provides its own loader over `echarts/core`, and specs provide
 * {@link fakeEchartsLoader} over the test kit's `FakeECharts`.
 */
export const PD_ECHARTS = new InjectionToken<() => Promise<PdEchartsLib>>('PD_ECHARTS', {
  providedIn: 'root',
  factory: () => async () => (await import('echarts')) as unknown as PdEchartsLib,
});

/** A loader over `FakeECharts.init` (`new FakeECharts().init`) for specs: `{ provide: PD_ECHARTS, useValue: fakeEchartsLoader(fake.init) }`. */
export function fakeEchartsLoader(init: () => PdChartInstance): () => Promise<PdEchartsLib> {
  return async () => ({ init: () => init(), registerTheme: () => undefined });
}

/** Whether the user (or the app) asked for less motion. */
export function reducedMotion(): boolean {
  const attr = document.documentElement.getAttribute('data-motion');
  if (attr === 'reduce') return true;
  if (attr === 'full') return false;
  return typeof matchMedia === 'function' && matchMedia('(prefers-reduced-motion: reduce)').matches;
}

/** The root attributes whose change means the colours (or the motion) must be read again. */
const APPEARANCE_ATTRIBUTES = ['data-theme', 'data-accent', 'data-contrast', 'data-motion', 'data-density'];

let nextId = 0;

/**
 * An Apache ECharts chart in the family's look (plan X21): the paddock theme from the tokens (palette, axes, tooltip, legend),
 * read again when the theme, accent or contrast changes (the chart is created again over the same option), resized with its box,
 * disposed with the component. Give it a `spec` (line, area, bar, stacked bar or donut) or a ready `option` of your own;
 * `label` names it and `summary` (default: the range and the latest value of every series) describes it in words, because a canvas
 * is invisible to a screen reader. Charts must mean something (UI rule 15): the spec carries the unit, bars start at zero, gaps
 * stay gaps and two or more series get a legend.
 *
 * ```html
 * <pd-chart label="Requests per hour" [spec]="requests()" [height]="220" (chartClick)="open($event)" />
 * ```
 */
@Component({
  selector: 'pd-chart',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [PdGlyph],
  host: { '[style.height.px]': 'hostHeight()' },
  template: `
    @if (showTitle()) {
      <h3 class="pd-title" data-testid="chart-title">{{ title() }}</h3>
    }
    <div #surface class="pd-surface" role="img" [hidden]="state() !== 'ready'" [attr.aria-label]="label()" [attr.aria-describedby]="summaryId"></div>
    @switch (state()) {
      @case ('loading') {
        <div class="pd-ghost" role="status" [attr.aria-label]="loadingText" data-testid="chart-loading">
          <span></span><span></span><span></span><span></span>
        </div>
      }
      @case ('error') {
        <div class="pd-error" role="alert" data-testid="chart-error">
          <pd-glyph name="alert" [size]="14" /><span>{{ errorText() || defaultError }}</span>
          <button type="button" class="pd-retry" (click)="retry.emit()">{{ retryText }}</button>
        </div>
      }
    }
    <p class="pd-sr" [id]="summaryId">{{ description() }}</p>
  `,
  styles: `
    :host { display: flex; flex-direction: column; position: relative; min-width: 0; width: 100%; }
    .pd-title { flex: none; margin: 0; padding: var(--pd-space-2) var(--pd-space-3) 0; font-size: var(--pd-text-body); font-weight: var(--pd-weight-semibold); color: var(--pd-text); }
    .pd-surface { flex: 1; min-height: 0; width: 100%; }
    .pd-surface[hidden] { display: none; }
    .pd-sr { position: absolute; width: 1px; height: 1px; overflow: hidden; clip-path: inset(50%); white-space: nowrap; margin: 0; }
    .pd-ghost { flex: 1; display: grid; align-content: space-evenly; padding: var(--pd-space-3); }
    .pd-ghost span { height: 1px; background: var(--pd-hairline); animation: pd-pulse 1.4s ease-in-out infinite; }
    @keyframes pd-pulse { 50% { opacity: 0.4; } }
    :root[data-motion='reduce'] .pd-ghost span { animation: none; }
    .pd-error { flex: 1; display: flex; flex-wrap: wrap; align-items: center; justify-content: center; gap: var(--pd-space-2); padding: var(--pd-space-3); color: var(--pd-text-muted); font-size: var(--pd-text-body); text-align: center; }
    .pd-error span { flex: 0 1 auto; max-width: 24rem; }
    .pd-error pd-glyph { color: var(--pd-danger-text, var(--pd-danger)); }
    .pd-retry { height: 24px; padding: 0 var(--pd-space-2); border: 1px solid var(--pd-border-control, var(--pd-hairline)); border-radius: var(--pd-radius-control); background: transparent; color: var(--pd-text); font: inherit; font-size: var(--pd-text-label); cursor: pointer; }
    .pd-retry:hover { background: var(--pd-hover); }
  `,
})
export class PdChart {
  /** Names the chart. */
  readonly label = input.required<string>();
  /** A generic chart. */
  readonly spec = input<PdChartSpec | null>(null);
  /** Your own ECharts option (wins over `spec`); build it from `readPaddockTokens` colours. */
  readonly option = input<Record<string, unknown> | null>(null);
  /** The words for people who cannot see the chart (default: from the spec). */
  readonly summary = input<string | null>(null);
  /** Height in pixels, title included. */
  readonly height = input(240);
  /** Draws `label` (and the unit) as the chart's visible title; turn it off when the card around it already has one. */
  readonly showTitle = input(true);
  /** `loading` shows ghosted axes, `error` the message with Retry; the chart is drawn when `ready`. */
  readonly state = input<'ready' | 'loading' | 'error'>('ready');
  /** What the error says (default: a plain sentence). */
  readonly errorText = input('');
  /** Retry was pressed in the error state. */
  readonly retry = output<void>();
  /** A series, slice or bar was clicked: ECharts' event, for the app's own handling. */
  readonly chartClick = output<unknown>();

  protected readonly summaryId = `pd-chart-${++nextId}-summary`;
  private readonly host = inject<ElementRef<HTMLElement>>(ElementRef).nativeElement;
  private readonly loader = inject(PD_ECHARTS);
  private readonly destroyRef = inject(DestroyRef);
  private readonly surface = viewChild.required<ElementRef<HTMLElement>>('surface');

  private lib: PdEchartsLib | null = null;
  private instance: PdChartInstance | null = null;
  private destroyed = false;
  private hidden = false;
  /** The tokens the chart is drawn with; null until the library is loaded. */
  private readonly tokens = signal<PaddockChartTokens | null>(null);
  /** Counts the times the instance was created, so the option is applied to each. */
  private readonly generation = signal(0);
  /** Counts appearance changes, so a spec is turned into an option again with the new motion setting. */
  private readonly appearance = signal(0);

  protected readonly loadingText = $localize`:@@pd.chart.loading:Loading chart`;
  protected readonly defaultError = $localize`:@@pd.chart.error:The chart could not be loaded.`;
  protected readonly retryText = $localize`:@@pd.chart.retry:Retry`;
  /** The visible title: the label, then the unit ("Requests per hour, req"). */
  protected readonly title = computed(() => {
    const [label, unit] = [this.label(), this.spec()?.unit];
    return unit && !label.includes(`(${unit})`) ? `${label} (${unit})` : label;
  });
  /** The box is narrower than 400 px: a donut puts its list under the ring. */
  private readonly narrow = signal(false);
  private readonly boxWidth = signal(320);
  private static readonly TITLE_PX = 30;
  /** The host's height: the asked one, or more for a narrow donut (ring on top, list under it, all inside the card). */
  protected readonly hostHeight = computed(() => {
    const spec = this.spec();
    const titleRoom = this.showTitle() ? PdChart.TITLE_PX : 0;
    const needed = spec?.kind === 'donut' && !this.option() && this.narrow() ? titleRoom + donutNarrowHeight(Math.min(spec.x.length, 5)) : 0;
    return Math.max(this.height(), needed);
  });
  protected readonly description = computed(() => {
    const spec = this.spec();
    return this.summary() ?? (spec ? chartSummary(spec) : '');
  });
  private readonly resolved = computed<Record<string, unknown> | null>(() => {
    const raw = this.option();
    if (raw) return raw;
    const [spec, tokens] = [this.spec(), this.tokens()];
    this.appearance();
    return spec && tokens ? chartOption(spec, tokens, { animate: !reducedMotion(), narrow: this.narrow(), width: this.boxWidth(), height: this.hostHeight() - (this.showTitle() ? PdChart.TITLE_PX : 0) }) : null;
  });

  constructor() {
    afterNextRender(() => void this.start());
    // The one side effect outside the signal graph: hand the option to the ECharts instance.
    effect(() => {
      const option = this.resolved();
      const ready = this.state() === 'ready';
      this.generation();
      if (option && this.instance) this.instance.setOption(option, { notMerge: true });
      // A chart that was hidden (loading, error) has no size to follow: measure it when it is shown again.
      if (ready && this.hidden) this.instance?.resize();
      this.hidden = !ready;
    });
    this.destroyRef.onDestroy(() => {
      this.destroyed = true;
      this.instance?.dispose();
      this.instance = null;
    });
  }

  private async start(): Promise<void> {
    const lib = await this.loader();
    if (this.destroyed) return;
    this.lib = lib;
    this.create();
    this.observe();
  }

  /** Creates the chart over the current tokens (again after an appearance change: ECharts themes apply at creation). */
  private create(): void {
    const lib = this.lib;
    if (!lib) return;
    this.instance?.dispose();
    const tokens = readPaddockTokens(this.host);
    const theme = registerPaddockTheme(lib, tokens);
    const instance = lib.init(this.surface().nativeElement, theme);
    instance.on('click', (e) => this.chartClick.emit(e));
    this.instance = instance;
    const width = Math.round(this.host.getBoundingClientRect().width);
    if (width > 0) this.measured(width);
    this.tokens.set(tokens);
    this.generation.update((n) => n + 1);
  }

  /** Remembers the box's width; the narrow layout starts under 400 px. */
  private measured(width: number): void {
    this.boxWidth.set(width);
    this.narrow.set(width < 400);
  }

  /** Follows the box (resize) and the appearance (colours). */
  private observe(): void {
    if (typeof ResizeObserver !== 'undefined') {
      const resize = new ResizeObserver((entries) => {
        const width = Math.round(entries[0]?.contentRect.width ?? 0);
        if (width > 0) this.measured(width);
        this.instance?.resize();
      });
      resize.observe(this.host);
      this.destroyRef.onDestroy(() => resize.disconnect());
    }
    if (typeof MutationObserver !== 'undefined') {
      const mutation = new MutationObserver(() => {
        this.appearance.update((n) => n + 1);
        this.create();
      });
      mutation.observe(document.documentElement, { attributes: true, attributeFilter: APPEARANCE_ATTRIBUTES });
      this.destroyRef.onDestroy(() => mutation.disconnect());
    }
  }
}
