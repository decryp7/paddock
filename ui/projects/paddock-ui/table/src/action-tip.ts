import { Directive, ElementRef, inject } from '@angular/core';
import { MatTooltip } from '@angular/material/tooltip';

const ACTION = 'button, a';

/**
 * One tooltip for a row's cluster of icon buttons (design guide: every icon-only button has an aria-label and a tooltip). The
 * tooltip sits to the left of the whole cluster, in its own row, so it never covers an action, and its message is the
 * `aria-label` of the button the pointer is over or the keyboard is on (the same pattern as a cell's full-text tooltip). The buttons inside
 * carry no `matTooltip` of their own.
 *
 * Part of `pd-table`: the row action cell of every table is one cluster with one tooltip.
 *
 * ```html
 * <span class="acts" pdActionTip> <button matIconButton aria-label="Edit rule">…</button> … </span>
 * ```
 */
@Directive({ selector: '[pdActionTip]', hostDirectives: [MatTooltip] })
export class PdActionTip {
  private readonly el = inject<ElementRef<HTMLElement>>(ElementRef).nativeElement;
  private readonly tip = inject(MatTooltip);

  constructor() {
    this.tip.position = 'left';
    // Capture listeners on the cluster run before the tooltip's own pointer listener, so the message is set in time. The pointer
    // moving from one button to the next only changes the message of the tooltip that is already open (a disabled button gets no
    // pointer events of its own in every browser, so the position is looked up on every move).
    this.el.addEventListener('mouseenter', (e) => this.sync(e), true);
    this.el.addEventListener('mousemove', (e) => this.sync(e), true);
    this.el.addEventListener('mouseleave', () => {
      if (!this.el.matches(':focus-within')) this.tip.message = '';
    });
    // The tooltip's own focus handling watches the host, not the buttons in it: the keyboard's focus is shown here.
    this.el.addEventListener('focusin', (e) => {
      if (this.sync(e) && e.target instanceof Element && e.target.matches(':focus-visible')) this.tip.show(0);
    });
    this.el.addEventListener('focusout', () => this.tip.hide(0));
  }

  /** Takes the name of the action an event is about; false when it is about none (the gap between two buttons). */
  private sync(e: Event): boolean {
    let button = e.target instanceof Element ? e.target.closest(ACTION) : null;
    // A disabled button does not always get pointer events itself: find the button under the pointer instead.
    if (!button && e instanceof MouseEvent) button = this.under(e.clientX, e.clientY);
    const label = button?.getAttribute('aria-label');
    if (!label || !this.el.contains(button)) return false;
    this.tip.message = label;
    return true;
  }

  private under(x: number, y: number): Element | null {
    for (const b of Array.from(this.el.querySelectorAll(ACTION))) {
      const r = b.getBoundingClientRect();
      if (r.width > 0 && x >= r.left && x <= r.right && y >= r.top && y <= r.bottom) return b;
    }
    return null;
  }
}
