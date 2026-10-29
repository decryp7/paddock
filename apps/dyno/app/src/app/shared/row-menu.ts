import { contentChildren, Directive, inject } from '@angular/core';
import { MatContextMenuTrigger } from '@angular/material/menu';
import { MatTooltip } from '@angular/material/tooltip';
import { PdContextMenuKeys } from '@decryp7/paddock-ui/material';

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
  private readonly tooltips = contentChildren(MatTooltip, { descendants: true });

  constructor() {
    // Whatever tooltip the pointer had open in the row closes at once (no delay, no animation wait), so the first Escape closes the menu.
    inject(MatContextMenuTrigger).menuOpened.subscribe(() => this.tooltips().forEach((t) => t.hide(0)));
  }
}
