import { afterNextRender, ChangeDetectionStrategy, Component, computed, DestroyRef, ElementRef, inject, InjectionToken, input, signal } from '@angular/core';

/** The width in pixels of `text` in `font`, or null when it cannot be measured (no canvas). */
export type PdMeasureText = (text: string, font: string) => number | null;

/** Measures with a canvas 2D context; null without one (jsdom), then the text is left to the CSS ellipsis. */
export const PD_MEASURE_TEXT = new InjectionToken<PdMeasureText>('PD_MEASURE_TEXT', {
  providedIn: 'root',
  factory: () => {
    let context: CanvasRenderingContext2D | null | undefined;
    return (text, font) => {
      if (context === undefined) context = typeof document === 'undefined' ? null : document.createElement('canvas').getContext('2d');
      if (!context) return null;
      context.font = font;
      return context.measureText(text).width;
    };
  },
});

/** The end of the text that is always kept: from the last `/` or `\` when that is short (a file name), else the last 14 characters. */
export function tailOf(text: string): string {
  const slash = Math.max(text.lastIndexOf('/'), text.lastIndexOf('\\'));
  return slash >= 0 && text.length - slash <= 28 ? text.slice(slash) : text.slice(-14);
}

/**
 * `text` shortened to fit, as ONE piece of text so the join looks the same in every row. A path is cut at segment boundaries and
 * keeps as many TRAILING segments as fit (`…/Debrief/plugins/viewer/plugin-1.bin`): the head is the part rows share, the end is the
 * part that tells them apart. Anything else (an id, a hash, a name without a separator), and a path whose last segment alone is too
 * wide, is cut in the middle by characters around the kept tail. `fits` says whether a candidate is narrow enough.
 */
export function middleTruncate(text: string, fits: (candidate: string) => boolean): string {
  if (fits(text)) return text;
  const parts = text.split(/(?<=[/\\])/);
  if (parts.length > 2) {
    // `parts` keep their trailing separator; drop leading parts until `…` + the rest fits.
    for (let from = 1; from < parts.length; from += 1) {
      const rest = parts.slice(from).join('');
      const separator = text[text.length - rest.length - 1] ?? '/';
      if (fits(`…${separator}${rest}`)) return `…${separator}${rest}`;
    }
  }
  return charTruncate(text, fits);
}

/** The character-level cut: the start, one ellipsis and the kept tail. */
function charTruncate(text: string, fits: (candidate: string) => boolean): string {
  const tail = tailOf(text);
  const room = text.length - tail.length;
  if (room <= 0) return text;
  let [lo, hi] = [0, room - 1];
  let best = `…${tail}`;
  while (lo <= hi) {
    const mid = (lo + hi) >> 1;
    const candidate = `${text.slice(0, mid)}…${tail}`;
    if (fits(candidate)) {
      best = candidate;
      lo = mid + 1;
    } else {
      hi = mid - 1;
    }
  }
  return best;
}

/**
 * A path, id or hash on one line, shortened in the middle (UI rule 9) to the width it is given, with the full value as the tooltip. Measures the
 * text in its own font, against the width it is GIVEN (`flex: 1 1 0`, never the width of the text it shows, or the box would shrink to the cut text and never grow back); where that is not possible it falls back to the CSS ellipsis at the end.
 */
@Component({
  selector: 'pd-middle-text',
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: { '[attr.title]': 'value()' },
  template: `{{ shown() }}`,
  styles: `
    :host { display: block; flex: 1 1 0; min-width: 0; width: 100%; overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
  `,
})
export class PdMiddleText {
  /** The text. */
  readonly value = input.required<string>();

  private readonly host = inject<ElementRef<HTMLElement>>(ElementRef).nativeElement;
  private readonly measure = inject(PD_MEASURE_TEXT);
  private readonly destroyRef = inject(DestroyRef);
  private readonly width = signal(0);
  protected readonly shown = computed(() => {
    const [text, width] = [this.value(), this.width()];
    if (width <= 0) return text;
    const font = getComputedStyle(this.host).font;
    return middleTruncate(text, (c) => (this.measure(c, font) ?? 0) <= width);
  });

  constructor() {
    afterNextRender(() => {
      this.width.set(Math.floor(this.host.getBoundingClientRect().width));
      if (typeof ResizeObserver === 'undefined') return;
      const observer = new ResizeObserver((entries) => this.width.set(Math.floor(entries[0]?.contentRect.width ?? this.width())));
      observer.observe(this.host);
      this.destroyRef.onDestroy(() => observer.disconnect());
    });
  }
}
