import { Directive, ElementRef, inject, input } from '@angular/core';
import { MatTooltip } from '@angular/material/tooltip';

/** The class of a full-text tip: its panel lets the pointer through (a list scrolled under a still pointer must keep the wheel). */
export const FULL_TEXT_TIP_CLASS = 'dy-full-text-tip';

/**
 * The full text of a clipped line, as a Material tooltip that exists only while the text is really cut off. The element already
 * shows (part of) this text, so a tooltip that is always present would be read twice by a screen reader (the tooltip becomes the
 * element's description); here the message is set only while the pointer is over a clipped element, and empty otherwise.
 */
@Directive({ selector: '[dyFullText]', hostDirectives: [MatTooltip], host: { '[attr.data-full-text]': 'dyFullText() || null' } })
export class FullTextTip {
  /** The text the element shows in full. */
  readonly dyFullText = input.required<string | null | undefined>();
  private readonly el = inject<ElementRef<HTMLElement>>(ElementRef).nativeElement;
  private readonly tip = inject(MatTooltip);

  constructor() {
    // The tip is a reading aid, not content to move onto: it never takes the pointer (see `.dy-full-text-tip` in styles.scss).
    this.tip.tooltipClass = FULL_TEXT_TIP_CLASS;
    // Capture listeners on the target run before the tooltip's own pointer listener, so the message is set in time.
    this.el.addEventListener('mouseenter', () => this.sync(), true);
    this.el.addEventListener('focusin', () => this.sync(), true);
    this.el.addEventListener('mouseleave', () => (this.tip.message = ''), true);
    this.el.addEventListener('focusout', () => (this.tip.message = ''), true);
  }

  private sync(): void {
    // While a menu is open (a row's context menu, say) the pointer's tooltips stay shut, so Escape closes the menu and nothing else.
    const menuOpen = document.querySelector('.mat-mdc-menu-panel') !== null;
    // Clipped across (an ellipsis) or down (a line clamp).
    const clipped = this.el.scrollWidth > this.el.clientWidth || this.el.scrollHeight > this.el.clientHeight;
    this.tip.message = clipped && !menuOpen ? (this.dyFullText() ?? '') : '';
  }
}
