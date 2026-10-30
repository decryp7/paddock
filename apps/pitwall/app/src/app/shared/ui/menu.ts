import { ChangeDetectionStrategy, Component, Directive, contentChildren, inject, input, viewChild } from '@angular/core';
import { MatIconButton } from '@angular/material/button';
import { MatIcon } from '@angular/material/icon';
import { MatContextMenuTrigger, MatMenu, MatMenuContent, MatMenuItem, MatMenuTrigger } from '@angular/material/menu';
import { MatTooltip } from '@angular/material/tooltip';
import { PdContextMenuKeys } from '@decryp7/paddock-ui/material';

/** One entry of a row's menu: the same list fills the "..." button's menu and the right-click menu. */
export interface MenuItem {
  label: string;
  run: () => void;
  /** Destructive: shown in the danger colour. The action behind it must ask for a confirmation that names the thing. */
  danger?: boolean;
  disabled?: boolean;
  /** Draws a divider above the item. */
  divider?: boolean;
}

/**
 * A Material menu (rule 47) whose items come with the trigger: `[matMenuTriggerData]="{ items }"` (or
 * `matContextMenuTriggerData`). One per list; every row's "..." button and right-click open it with that row's items.
 *
 * ```html
 * <pw-items-menu #rowMenu label="Model actions" />
 * <tr pwRowMenu [matContextMenuTriggerFor]="rowMenu.menu()" [matContextMenuTriggerData]="{ items: menuItems(m) }">…
 * ```
 */
@Component({
  selector: 'pw-items-menu',
  imports: [MatMenu, MatMenuContent, MatMenuItem],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <mat-menu [aria-label]="label()" class="pw-items-menu">
      <ng-template matMenuContent let-items="items">
        @for (item of asItems(items); track item.label) {
          @if (item.divider) {
            <hr class="pw-menu-divider" />
          }
          <button type="button" mat-menu-item [class.danger]="item.danger" [disabled]="!!item.disabled" (click)="item.run()">{{ item.label }}</button>
        }
      </ng-template>
    </mat-menu>
  `,
})
export class ItemsMenu {
  /** The menu's accessible name ("Model actions"). */
  readonly label = input('Actions');
  /** The panel the triggers point at. */
  readonly menu = viewChild.required(MatMenu);

  protected asItems(items: unknown): readonly MenuItem[] {
    return Array.isArray(items) ? (items as MenuItem[]) : [];
  }
}

/**
 * A row's context menu (rule 47: Material's `matContextMenuTriggerFor`): opens at the pointer on a right-click, and from the
 * keyboard on Shift+F10 or the Menu key (`pdContextMenuKeys`; macOS sends no `contextmenu` for them), and gives focus back to
 * the row on close. A tooltip the pointer opened in the row closes first, so the first Escape closes the menu.
 * Dyno's `dyRowMenu` is the same directive; paddock-ui has none yet (reported as a gap of WP-2.3).
 */
@Directive({
  selector: '[pwRowMenu]',
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
    inject(MatContextMenuTrigger).menuOpened.subscribe(() => this.tooltips().forEach((t) => t.hide(0)));
  }
}

/**
 * The "..." icon button at the end of a row: it stays in the row in `--pd-row-action-rest` (3:1 or better; keyboard and touch can
 * always reach it), takes the full text colour on row hover, focus or while open (design guide, Tables), and opens the row's menu (the same list as the right-click menu). Destructive items
 * live only there.
 */
@Component({
  selector: 'pw-more-button',
  imports: [MatIconButton, MatIcon, MatMenuTrigger, MatTooltip],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <button type="button" matIconButton class="pw-more" [matMenuTriggerFor]="menu()" [matMenuTriggerData]="{ items: items() }" [attr.aria-label]="label()" matTooltip="More actions" (click)="$event.stopPropagation()">
      <mat-icon svgIcon="pw:more" aria-hidden="true" />
    </button>
  `,
  styles: `
    :host {
      display: inline-flex;
    }
    .pw-more {
      --mat-icon-button-icon-color: var(--pd-row-action-rest);
    }
    :host-context(tr:hover) .pw-more,
    :host-context(.row:hover) .pw-more,
    :host-context([role='row']:hover) .pw-more,
    .pw-more:focus-visible,
    .pw-more[aria-expanded='true'] {
      --mat-icon-button-icon-color: var(--pd-text);
    }
  `,
})
export class MoreButton {
  /** The list's menu (`pw-items-menu`'s `menu()`). */
  readonly menu = input.required<MatMenu>();
  readonly items = input.required<readonly MenuItem[]>();
  /** Names the row: "More actions for Qwen3.5 9B". */
  readonly label = input('More actions');
}
