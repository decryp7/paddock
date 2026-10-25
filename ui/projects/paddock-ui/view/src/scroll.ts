import { afterNextRender, DestroyRef, Directive, ElementRef, inject, signal } from '@angular/core';

/**
 * CSS for a region that scrolls on its own: the side that has more to show fades (`data-more` is `start`, `end` or `both`), so a wide
 * table, matrix or drawing says that it continues. The focus ring of the region is drawn inside it (a mask clips anything outside).
 */
export const SCROLL_CSS = `
  /* The region is the containing block of the visually hidden text inside it, so that text scrolls with the region and cannot widen the page. */
  [pdViewScroll] { position: relative; }
  [pdViewScroll]:focus-visible { outline: 2px solid var(--pd-focus-ring); outline-offset: -2px; }
  [pdViewScroll][data-more='end'] { -webkit-mask-image: linear-gradient(to right, #000 calc(100% - 28px), transparent); mask-image: linear-gradient(to right, #000 calc(100% - 28px), transparent); }
  [pdViewScroll][data-more='start'] { -webkit-mask-image: linear-gradient(to left, #000 calc(100% - 28px), transparent); mask-image: linear-gradient(to left, #000 calc(100% - 28px), transparent); }
  [pdViewScroll][data-more='both'] { -webkit-mask-image: linear-gradient(to right, transparent, #000 28px, #000 calc(100% - 28px), transparent); mask-image: linear-gradient(to right, transparent, #000 28px, #000 calc(100% - 28px), transparent); }
`;

/**
 * Marks a region that scrolls on its own (`overflow: auto`): it is a tab stop only while it really overflows, so a keyboard user can
 * scroll it, and a table that fits adds no stop. Watches the region and its first child (the table or drawing) for size changes, and
 * says which side has more to show (`data-more`, see {@link SCROLL_CSS}).
 */
@Directive({ selector: '[pdViewScroll]', host: { '[attr.tabindex]': 'overflowing() ? 0 : null', '[attr.data-more]': 'more()', '(scroll)': 'measure()' } })
export class PdViewScroll {
  private readonly element = inject<ElementRef<HTMLElement>>(ElementRef).nativeElement;
  private readonly destroyRef = inject(DestroyRef);
  protected readonly overflowing = signal(false);
  /** Which side continues: `start`, `end`, `both`, or null when everything is in view. */
  protected readonly more = signal<'start' | 'end' | 'both' | null>(null);

  constructor() {
    afterNextRender(() => {
      if (typeof ResizeObserver === 'undefined') return;
      const watch = new ResizeObserver(() => this.measure());
      watch.observe(this.element);
      if (this.element.firstElementChild) watch.observe(this.element.firstElementChild);
      this.measure();
      this.destroyRef.onDestroy(() => watch.disconnect());
    });
  }

  protected measure(): void {
    const e = this.element;
    this.overflowing.set(e.scrollWidth > e.clientWidth + 1 || e.scrollHeight > e.clientHeight + 1);
    const left = Math.abs(e.scrollLeft);
    const start = left > 1;
    const end = e.scrollWidth > e.clientWidth + 1 && left + e.clientWidth < e.scrollWidth - 1;
    this.more.set(start && end ? 'both' : start ? 'start' : end ? 'end' : null);
  }
}
