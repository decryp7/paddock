import { ChangeDetectionStrategy, Component, DestroyRef, ElementRef, afterNextRender, inject, input } from '@angular/core';
import { BACKEND } from '../core/backend';

/**
 * A native-looking popover surface for the tray / menu-bar window (reusable: Pitwall and the other apps copy this file
 * and `.dy-popover` styles). The page behind it is transparent (`<html data-surface="popover">`, set by the page); the
 * Rust shell supplies the material and the rounded window. The card is a hairline-bordered, semi-transparent sheet with
 * a 12 px radius whose height follows its content up to `maxHeight` (then it scrolls inside). Every layout change is
 * measured and reported with `backend.popoverResize(height)` so the shell can size the window to fit.
 *
 * Open and close animate (120 ms fade and scale) from `data-popover-open` on `<html>`: absent or `true` is open,
 * `false` is the closed state the shell sets just before hiding. Reduced motion removes the animation.
 */
@Component({
  selector: 'dy-popover',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `<div class="sheet" #sheet><ng-content /></div>`,
  styles: `
    :host { display: block; }
    .sheet {
      max-height: var(--popover-max, 560px);
      overflow-x: hidden;
      overflow-y: auto;
      border-radius: var(--popover-radius, 12px);
      border: 1px solid var(--popover-border);
      background: var(--popover-bg);
      box-shadow: var(--popover-shadow);
      backdrop-filter: blur(24px) saturate(1.4);
      -webkit-backdrop-filter: blur(24px) saturate(1.4);
      transform-origin: top center;
      transition: opacity 120ms var(--ease-out), transform 120ms var(--ease-out);
    }
    :host-context(html[data-popover-open='false']) .sheet { opacity: 0; transform: scale(0.97); }
    @media (prefers-reduced-motion: reduce) {
      .sheet { transition: none; }
      :host-context(html[data-popover-open='false']) .sheet { transform: none; }
    }
  `,
})
export class Popover {
  /** The tallest the popover may grow, in CSS pixels. */
  readonly maxHeight = input(560);
  private readonly el = inject<ElementRef<HTMLElement>>(ElementRef);
  private readonly backend = inject(BACKEND);
  private last = -1;

  constructor() {
    const destroyRef = inject(DestroyRef);
    afterNextRender(() => {
      const sheet = this.el.nativeElement.querySelector<HTMLElement>('.sheet');
      if (!sheet) return;
      sheet.style.setProperty('--popover-max', `${this.maxHeight()}px`);
      const report = () => {
        // The sheet's border box, rounded up, is what the window must fit (a failure to resize is not the page's problem).
        const h = Math.ceil(sheet.getBoundingClientRect().height);
        if (h === this.last || h <= 0) return;
        this.last = h;
        this.backend.popoverResize(h).catch(() => undefined);
      };
      report();
      if (typeof ResizeObserver === 'undefined') return;
      // The sheet scrolls inside at max-height, so watch its content too.
      const observer = new ResizeObserver(report);
      observer.observe(sheet);
      for (const child of Array.from(sheet.children)) observer.observe(child);
      destroyRef.onDestroy(() => observer.disconnect());
    });
  }
}
