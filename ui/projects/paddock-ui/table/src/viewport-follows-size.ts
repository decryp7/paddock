import { CdkVirtualScrollViewport } from '@angular/cdk/scrolling';
import { DestroyRef, Directive, ElementRef, inject } from '@angular/core';

/**
 * Makes a CDK virtual-scroll viewport measure itself again whenever its element changes size.
 *
 * The CDK measures a viewport's height at start and on a window resize only. A viewport that changes height later (a widget
 * resized by its grip, rows arriving after a skeleton, a list that grows) would draw only the rows its old height covered and
 * leave the rest blank. Put this on every `cdk-virtual-scroll-viewport` whose height can change without the window resizing.
 */
@Directive({ selector: 'cdk-virtual-scroll-viewport[pdViewportFollowsSize]' })
export class PdViewportFollowsSize {
  constructor() {
    const viewport = inject(CdkVirtualScrollViewport);
    const observer = typeof ResizeObserver === 'undefined' ? null : new ResizeObserver(() => viewport.checkViewportSize());
    observer?.observe(inject<ElementRef<HTMLElement>>(ElementRef).nativeElement);
    inject(DestroyRef).onDestroy(() => observer?.disconnect());
  }
}
