import { MatTooltip } from '@angular/material/tooltip';
import { ChangeDetectionStrategy, Component, computed, ElementRef, effect, input, signal, viewChild } from '@angular/core';
import { fitEnd, fitPath } from '@decryp7/paddock-ui/format';
import { PdGlyph } from '@decryp7/paddock-ui/icons';
import { copyText } from './clipboard';

/**
 * A path, file name or other long identifier on one line that shortens in the middle when its container is narrow (rule 18).
 * The text is fitted to the measured width (`fitPath`), so there is one ellipsis and no gap beside it: the root ("C:\" or "/")
 * stays, then the start of the directory, the ellipsis, the separator and the file name. When the name alone is too wide it shrinks
 * in the middle and keeps its last `keepTail` characters (the extension and the last word). Set in the
 * mono face at 12 px (`plain` for ordinary text). The full value is a `matTooltip`, shown only when the text on screen is not the whole value, and, with `copy`, one click on a Copy button (shown on hover and focus). Fills the
 * width it is given: put it in a flex or grid child with `min-width: 0`.
 */
@Component({
  selector: 'pd-path-text',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [PdGlyph, MatTooltip],
  template: `<span #box class="pd-text" [class.pd-plain]="plain()" [matTooltip]="whole()" [matTooltipDisabled]="!clipped()" [attr.data-full]="whole()" data-testid="path-text"><span class="pd-size" aria-hidden="true" [attr.data-full]="value()"></span><span class="pd-shown">{{ shown() }}</span></span>
    @if (copy()) {
      <button class="pd-copy" type="button" [attr.aria-label]="copied() ? copiedLabel : copyLabel" [matTooltip]="copied() ? copiedLabel : copyLabel" (click)="doCopy($event)">
        <pd-glyph [name]="copied() ? 'check' : 'copy'" [size]="14" />
      </button>
    }`,
  styles: `
    :host { display: flex; align-items: center; gap: var(--pd-space-1); min-width: 0; max-width: 100%; }
    /* The box is as wide as the whole value or as the room it has, whichever is less: the hidden sizing line carries the whole value, the shown line the fitted one. */
    .pd-text { position: relative; display: block; min-width: 0; flex: 0 1 auto; overflow: hidden; white-space: nowrap; font-family: var(--pd-font-mono); font-size: var(--pd-text-label); }
    .pd-text.pd-plain { font-family: inherit; font-size: inherit; }
    .pd-size { display: block; visibility: hidden; height: 1lh; overflow: hidden; }
    .pd-size::before { content: attr(data-full); }
    .pd-shown { position: absolute; inset: 0 auto 0 0; white-space: pre; }
    .pd-copy { flex: none; display: inline-flex; padding: 2px; border: 0; border-radius: var(--pd-radius-tag); background: transparent; color: var(--pd-text-muted); cursor: pointer; opacity: 0; }
    :host(:hover) .pd-copy, .pd-copy:focus-visible { opacity: 1; }
    .pd-copy:hover { color: var(--pd-text); background: var(--pd-hover); }
    .pd-copy:focus-visible { outline: 2px solid var(--pd-focus-ring); }
    @media (hover: none) { .pd-copy { opacity: 0.6; } }
  `,
})
export class PdPathText {
  /** The text shown (shortened in the middle when it does not fit). */
  readonly value = input.required<string>();
  /** What the tooltip shows and Copy copies when it is more than `value` (a file name shown for its full path). */
  readonly full = input<string | null>(null);
  /** Characters of the end that are never cut (a longer file name keeps its last ones). */
  readonly keepTail = input(20);
  /** Ordinary text face instead of the mono face paths, hashes and addresses use. */
  readonly plain = input(false);
  /** Where the cut falls: `middle` (paths: the directory, then the start of the name) or `end` (a title: the end of the text). */
  readonly clip = input<'middle' | 'end'>('middle');
  /** Shows a Copy button for the full value. */
  readonly copy = input(false);

  private readonly box = viewChild.required<ElementRef<HTMLElement>>('box');
  /** The width the text may use, measured from the box (0 until it is laid out: the whole value is shown then). */
  private readonly width = signal(0);
  private canvas: CanvasRenderingContext2D | null | undefined;
  /** The value fitted to the measured width. */
  protected readonly shown = computed(() => {
    const width = this.width();
    const value = this.value();
    if (width <= 0) return value;
    const measure = this.measurer();
    if (!measure) return value;
    return this.clip() === 'end' ? fitEnd(value, width, measure) : fitPath(value, width, measure, this.keepTail());
  });
  protected readonly whole = computed(() => this.full() ?? this.value());
  /** What is drawn is less than the whole value (cut to fit, or a short name for a full path): only then is the tooltip worth showing. */
  protected readonly clipped = computed(() => this.shown() !== this.whole());
  protected readonly copied = signal(false);

  constructor() {
    // effect-state: a ResizeObserver is outside the signal graph (DOM); it sets the width signal because the browser, not another signal, owns that value.
    effect((onCleanup) => {
      const el = this.box().nativeElement;
      if (typeof ResizeObserver === 'undefined') return;
      const watch = new ResizeObserver(() => this.width.set(Math.ceil(el.getBoundingClientRect().width)));
      watch.observe(el);
      onCleanup(() => watch.disconnect());
    });
  }

  /** Text width in pixels in the box's face, or null where a canvas is not available (the whole value is shown). */
  private measurer(): ((s: string) => number) | null {
    const el = this.box().nativeElement;
    if (this.canvas === undefined) {
      try {
        this.canvas = document.createElement('canvas').getContext('2d');
      } catch {
        this.canvas = null;
      }
    }
    const ctx = this.canvas;
    if (!ctx) return null;
    const style = getComputedStyle(el);
    ctx.font = `${style.fontStyle} ${style.fontWeight} ${style.fontSize} ${style.fontFamily}`;
    return (s) => ctx.measureText(s).width;
  }
  protected readonly copyLabel = $localize`:@@pd.pathText.copy:Copy the full path`;
  protected readonly copiedLabel = $localize`:@@pd.pathText.copied:Copied`;

  protected async doCopy(event: Event): Promise<void> {
    event.stopPropagation();
    const text = this.whole();
    if (!(await copyText(text))) return;
    this.copied.set(true);
    // Resetting the label is a side effect on this component's own display, a timer outside the signal graph.
    setTimeout(() => this.copied.set(false), 1500);
  }
}
