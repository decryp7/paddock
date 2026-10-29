import { Directive, ElementRef, inject } from '@angular/core';
import { MatContextMenuTrigger } from '@angular/material/menu';
import { PdContextMenuKeys } from '@decryp7/paddock-ui';

/**
 * A row's context menu (rule 47: Material's `matContextMenuTriggerFor`): opens at the pointer on a right-click, and from the
 * keyboard on Shift+F10 or the Menu key (`pdContextMenuKeys`; macOS sends no `contextmenu` for them), and gives focus back to
 * the row on close. A tooltip that the pointer opened is closed first, so the first Escape closes the menu and not the tooltip.
 *
 * ```html
 * <tr tabindex="0" dyRowMenu [matContextMenuTriggerFor]="menu" [matContextMenuTriggerData]="{ row }">
 * ```
 */
@Directive({
  selector: '[dyRowMenu]',
  hostDirectives: [
    {
      directive: MatContextMenuTrigger,
      inputs: ['matContextMenuTriggerFor', 'matContextMenuTriggerData', 'matContextMenuTriggerDisabled'],
      outputs: ['menuOpened', 'menuClosed'],
    },
    PdContextMenuKeys,
  ],
})
export class RowMenu {
  constructor() {
    const host = inject<ElementRef<HTMLElement>>(ElementRef).nativeElement;
    // Capture phase: before the trigger opens the menu, tell the tooltips of what the pointer is over that it left.
    host.addEventListener(
      'contextmenu',
      (e) => {
        for (let el = e.target instanceof Element ? e.target : null; el; el = el === host ? null : el.parentElement) {
          el.dispatchEvent(new MouseEvent('mouseleave', { relatedTarget: null }));
        }
      },
      true,
    );
  }
}
