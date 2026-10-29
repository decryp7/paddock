import { Directive } from '@angular/core';

/** The keyboard's right-click: the Menu (ContextMenu) key, or Shift+F10. */
export function isContextMenuKey(e: KeyboardEvent): boolean {
  return e.key === 'ContextMenu' || (e.key === 'F10' && e.shiftKey);
}

/**
 * Opens a row's context menu from the keyboard. Not every platform turns the Menu key or Shift+F10 into a `contextmenu` event
 * (macOS does not), so a focused host gets one here, at its left edge and middle, for whatever `matContextMenuTriggerFor` (or any
 * `contextmenu` listener) is on it. Only a key pressed on the host itself counts: the same keys inside one of its buttons or
 * fields are left alone.
 *
 * ```html
 * <tr tabindex="0" pdContextMenuKeys [matContextMenuTriggerFor]="menu" [matContextMenuTriggerData]="{ row }">…</tr>
 * ```
 */
@Directive({
  selector: '[pdContextMenuKeys]',
  host: { '(keydown)': 'onKeydown($event)' },
})
export class PdContextMenuKeys {
  protected onKeydown(e: KeyboardEvent): void {
    const host = e.currentTarget;
    if (!(host instanceof HTMLElement) || e.target !== host || !isContextMenuKey(e)) return;
    e.preventDefault();
    const box = host.getBoundingClientRect();
    host.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: box.left + 24, clientY: box.top + box.height / 2 }));
  }
}
