import { ChangeDetectionStrategy, Component, ElementRef, inject, input } from '@angular/core';
import { MatIconModule } from '@angular/material/icon';
import { MatTooltipModule } from '@angular/material/tooltip';
import { ContextMenuService, type ContextMenuItem } from './context-menu';

/**
 * The "..." button at the end of a row. It stays in the row at 40 % opacity (keyboard and touch can always reach it)
 * and opens the same list as the row's right-click menu. Destructive items are `danger` and only live here.
 */
@Component({
  selector: 'pw-row-menu',
  imports: [MatIconModule, MatTooltipModule],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <button type="button" class="btn ghost icon sm more" [attr.aria-label]="label()" aria-haspopup="menu" matTooltip="More actions" (click)="open($event)">
      <mat-icon svgIcon="pw:more" aria-hidden="true" />
    </button>
  `,
  styles: `
    :host {
      display: inline-flex;
    }
    .more {
      opacity: 0.4;
    }
    :host-context(tr:hover) .more,
    :host-context(.row:hover) .more,
    .more:focus-visible,
    .more[aria-expanded='true'] {
      opacity: 1;
    }
  `,
})
export class RowMenu {
  readonly items = input.required<ContextMenuItem[]>();
  readonly label = input('More actions');
  private readonly menu = inject(ContextMenuService);
  private readonly host = inject<ElementRef<HTMLElement>>(ElementRef);

  protected open(e: Event): void {
    e.stopPropagation();
    const button = this.host.nativeElement.querySelector('button');
    if (button) this.menu.openFrom(button, this.items());
  }
}
