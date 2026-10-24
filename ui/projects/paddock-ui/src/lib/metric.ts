import { ChangeDetectionStrategy, Component, computed, input } from '@angular/core';

export type SuMetricSize = 'display' | 'title' | 'body';
/** Which direction of change is an improvement; `none` keeps the delta neutral (calm by default). */
export type SuGoodDirection = 'up' | 'down' | 'none';
export type SuDeltaTone = 'good' | 'bad' | 'neutral';

/** Tone of a delta: colour only appears when the caller says which direction is good. */
export function deltaTone(delta: number, good: SuGoodDirection): SuDeltaTone {
  if (delta === 0 || good === 'none') return 'neutral';
  return (delta > 0) === (good === 'up') ? 'good' : 'bad';
}

/** Formats a number with thousands separators and a fixed number of decimals (when given). */
export function formatNumber(value: number, decimals: number | undefined, locale?: string): string {
  if (!Number.isFinite(value)) return '—';
  const options: Intl.NumberFormatOptions =
    decimals === undefined ? { maximumFractionDigits: 2 } : { minimumFractionDigits: decimals, maximumFractionDigits: decimals };
  return new Intl.NumberFormat(locale, options).format(value);
}

/** A number as the hero of a card: tabular, the unit a smaller muted sibling, an optional delta with arrow and sign. */
@Component({
  selector: 'pd-metric',
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: { '[class]': '"pd-metric pd-metric--" + size()' },
  template: `
    <span class="pd-line">
      <span class="pd-value">{{ text() }}</span>
      @if (unit()) {
        <span class="pd-unit">{{ unit() }}</span>
      }
    </span>
    @if (hasDelta()) {
      <span [class]="'pd-delta pd-delta--' + tone()">
        <span class="pd-visually-hidden">{{ deltaAria() }}</span>
        <svg viewBox="0 0 12 12" width="10" height="10" aria-hidden="true" focusable="false">
          <path [attr.d]="arrowPath()" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" />
        </svg>
        <span aria-hidden="true">{{ deltaText() }}</span>
        @if (deltaLabel()) {
          <span class="pd-delta-label" aria-hidden="true">{{ deltaLabel() }}</span>
        }
      </span>
    }
  `,
  styles: `
    :host { display: inline-flex; flex-direction: column; gap: var(--pd-space-1); font-variant-numeric: tabular-nums; }
    .pd-line { display: inline-flex; align-items: baseline; gap: var(--pd-space-1); }
    .pd-value { font-weight: var(--pd-weight-semibold); color: var(--pd-text); }
    .pd-unit { color: var(--pd-text-muted); font-size: var(--pd-text-body); font-weight: var(--pd-weight-regular); }
    :host(.pd-metric--display) .pd-value { font-size: var(--pd-text-display); line-height: var(--pd-lh-display); }
    :host(.pd-metric--title) .pd-value { font-size: var(--pd-text-title); line-height: var(--pd-lh-title); }
    :host(.pd-metric--body) .pd-value { font-size: var(--pd-text-body); line-height: var(--pd-lh-body); }
    :host(.pd-metric--body) .pd-unit { font-size: var(--pd-text-label); }
    .pd-delta { display: inline-flex; align-items: center; gap: var(--pd-space-1); font-size: var(--pd-text-label); line-height: var(--pd-lh-label); color: var(--pd-text-muted); }
    .pd-delta--good { color: var(--pd-success); }
    .pd-delta--bad { color: var(--pd-danger); }
    .pd-delta-label { color: var(--pd-text-muted); }
    .pd-visually-hidden { position: absolute; width: 1px; height: 1px; margin: -1px; overflow: hidden; clip: rect(0 0 0 0); white-space: nowrap; }
  `,
})
export class SuMetric {
  /** The number (formatted with thousands separators) or a ready-made string. */
  readonly value = input.required<number | string | null>();
  readonly unit = input<string>('');
  /** Fixed decimals for numeric values; omitted: up to 2. */
  readonly decimals = input<number | undefined>(undefined);
  readonly size = input<SuMetricSize>('display');
  /** Change against the comparison period, in the same unit as the delta label (e.g. percent points). */
  readonly delta = input<number | null>(null);
  readonly deltaUnit = input<string>('');
  /** What the delta is against, e.g. "vs last week". */
  readonly deltaLabel = input<string>('');
  readonly goodDirection = input<SuGoodDirection>('none');
  readonly locale = input<string | undefined>(undefined);

  protected readonly text = computed(() => {
    const v = this.value();
    if (v === null) return '—';
    return typeof v === 'number' ? formatNumber(v, this.decimals(), this.locale()) : v;
  });
  protected readonly hasDelta = computed(() => this.delta() !== null);
  protected readonly tone = computed(() => deltaTone(this.delta() ?? 0, this.goodDirection()));
  protected readonly deltaText = computed(() => {
    const d = this.delta() ?? 0;
    const sign = d > 0 ? '+' : d < 0 ? '−' : '';
    return `${sign}${formatNumber(Math.abs(d), undefined, this.locale())}${this.deltaUnit()}`;
  });
  protected readonly arrowPath = computed(() => {
    const d = this.delta() ?? 0;
    return d > 0 ? 'M6 10V2M2.5 5.5 6 2l3.5 3.5' : d < 0 ? 'M6 2v8M2.5 6.5 6 10l3.5-3.5' : 'M2 6h8';
  });
  protected readonly deltaAria = computed(() => {
    const d = this.delta() ?? 0;
    const dir = d > 0 ? $localize`:@@pd.ui.metric.up:Up` : d < 0 ? $localize`:@@pd.ui.metric.down:Down` : $localize`:@@pd.ui.metric.same:No change`;
    const amount = d === 0 ? '' : ` ${formatNumber(Math.abs(d), undefined, this.locale())}${this.deltaUnit()}`;
    return `${dir}${amount}${this.deltaLabel() ? ' ' + this.deltaLabel() : ''}`;
  });
}
