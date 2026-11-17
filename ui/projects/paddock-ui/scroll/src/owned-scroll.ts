import { Directive } from '@angular/core';

/**
 * Marks a scroller whose position our code sets (a virtual list or grid, a live log or tail, anything that writes `scrollTop` or
 * `scrollLeft` or calls `scrollTo`, `scrollBy` or a CDK `scrollToOffset`): the browser makes no scroll moves of its own on it (family ADR
 * 0105).
 *
 * - `overflow-anchor: none`: CSS scroll anchoring (Chromium, and WebKit including WKWebView) moves `scrollTop` by itself when content
 *   above its anchor changes size; a scroller that places its rows itself then sees a scroll it did not write (Debrief's events grid: two
 *   rows off after a resize).
 * - `scroll-behavior: auto`: a write lands at once, so a read right after it returns the written position.
 *
 * The Angular CDK sets the same two properties on its virtual scroll viewport (`.cdk-virtual-scrollable`). The styles are bound by Angular
 * (CSSOM, allowed by the CSP), so the directive works in an app that does not load `paddock.css`; the `pd-owned-scroll` class is the same
 * rule for stylesheets (`_base.scss`, and the `owned-scroll` mixin).
 */
@Directive({
  selector: '[pdOwnedScroll]',
  host: {
    class: 'pd-owned-scroll',
    '[style.overflow-anchor]': '"none"',
    '[style.scroll-behavior]': '"auto"',
  },
})
export class PdOwnedScroll {}
