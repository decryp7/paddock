import { afterNextRender, DestroyRef, Directive, ElementRef, inject, signal } from '@angular/core';

/**
 * Marks a region that scrolls on its own (`overflow: auto`): it is a tab stop only while it really overflows, so a keyboard user can
 * scroll it, and a table that fits adds no stop. Watches the region and its first child (the table or drawing) for size changes.
 */
@Directive({ selector: '[pdViewScroll]', host: { '[attr.tabindex]': 'overflowing() ? 0 : null' } })
export class PdViewScroll {
  private readonly element = inject<ElementRef<HTMLElement>>(ElementRef).nativeElement;
  private readonly destroyRef = inject(DestroyRef);
  protected readonly overflowing = signal(false);

  constructor() {
    afterNextRender(() => {
      if (typeof ResizeObserver === 'undefined') return;
      const measure = (): void => this.overflowing.set(this.element.scrollWidth > this.element.clientWidth + 1 || this.element.scrollHeight > this.element.clientHeight + 1);
      const watch = new ResizeObserver(measure);
      watch.observe(this.element);
      if (this.element.firstElementChild) watch.observe(this.element.firstElementChild);
      measure();
      this.destroyRef.onDestroy(() => watch.disconnect());
    });
  }
}
