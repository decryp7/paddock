import { Directive, ElementRef, inject, input } from '@angular/core';
import { MatTooltip } from '@angular/material/tooltip';

/**
 * The full text of a clipped line, as a Material tooltip that exists only while the text is really cut off. The element already
 * shows (part of) this text, so a tooltip that is always present would be read twice by a screen reader (the tooltip becomes the
 * element's description); here the message is set only while the pointer is over a clipped element, and empty otherwise.
 */
@Directive({ selector: '[dyFullText]', hostDirectives: [MatTooltip] })
export class FullTextTip {
  /** The text the element shows in full. */
  readonly dyFullText = input.required<string | null | undefined>();
  private readonly el = inject<ElementRef<HTMLElement>>(ElementRef).nativeElement;
  private readonly tip = inject(MatTooltip);

  constructor() {
    // Capture listeners on the target run before the tooltip's own pointer listener, so the message is set in time.
    this.el.addEventListener('mouseenter', () => this.sync(), true);
    this.el.addEventListener('focusin', () => this.sync(), true);
    this.el.addEventListener('mouseleave', () => (this.tip.message = ''), true);
    this.el.addEventListener('focusout', () => (this.tip.message = ''), true);
  }

  private sync(): void {
    // While a menu is open (a row's context menu, say) the pointer's tooltips stay shut, so Escape closes the menu and nothing else.
    const menuOpen = document.querySelector('.mat-mdc-menu-panel') !== null;
    const clipped = this.el.scrollWidth > this.el.clientWidth;
    this.tip.message = clipped && !menuOpen ? (this.dyFullText() ?? '') : '';
  }
}
