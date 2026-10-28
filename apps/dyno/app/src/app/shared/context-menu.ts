import type { MatMenuTrigger } from '@angular/material/menu';

/**
 * Opens a Material menu at the pointer for a right-click or the keyboard's menu key (both arrive as `contextmenu`). `mat-menu`
 * opens from a trigger element, so each list keeps one zero-size anchor (`.dy-ctx-anchor`, fixed) with its own `matMenuTriggerFor`:
 * the anchor is moved to the pointer, the menu's template data is set, and the menu opens there. Put the anchor and the trigger
 * on one element with template references and call this from the row's `(contextmenu)`.
 */
export function openContextMenu(event: MouseEvent, anchor: HTMLElement, trigger: MatMenuTrigger, data: unknown): void {
  event.preventDefault();
  event.stopPropagation();
  if (trigger.menuOpen) trigger.closeMenu();
  // The keyboard's menu key reports (0, 0): open at the row instead.
  const at = event.clientX === 0 && event.clientY === 0 && event.target instanceof HTMLElement ? event.target.getBoundingClientRect() : null;
  anchor.style.left = `${at ? at.left : event.clientX}px`;
  anchor.style.top = `${at ? at.bottom : event.clientY}px`;
  trigger.menuData = data;
  trigger.openMenu();
}
