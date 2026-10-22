import { CdkMenu, CdkMenuItem, CdkMenuTrigger } from '@angular/cdk/menu';
import { ChangeDetectionStrategy, Component, input, output } from '@angular/core';
import { MatIcon } from '@angular/material/icon';

/** One entry of a "..." menu. */
export interface MenuEntry {
  id: string;
  label: string;
  icon?: string;
  /** Danger text; the caller confirms before doing it. */
  danger?: boolean;
  /** A hairline above the entry. */
  separated?: boolean;
  disabled?: boolean;
}

/** The "..." button with its menu (CDK menu: arrow keys, Esc, focus return). */
@Component({
  selector: 'sp-menu-button',
  imports: [MatIcon, CdkMenu, CdkMenuItem, CdkMenuTrigger],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <button type="button" class="btn ghost icon more" [class.sm]="small()" [cdkMenuTriggerFor]="menu" [attr.aria-label]="label()" [attr.title]="label()" (click)="$event.stopPropagation()">
      <mat-icon svgIcon="sp:more" aria-hidden="true" />
    </button>
    <ng-template #menu>
      <div class="menu" cdkMenu>
        @for (e of entries(); track e.id) {
          @if (e.separated) { <div class="menu-sep" role="separator"></div> }
          <button type="button" class="menu-item" [class.danger]="e.danger" cdkMenuItem [cdkMenuItemDisabled]="!!e.disabled" (cdkMenuItemTriggered)="picked.emit(e.id)">
            @if (e.icon) { <mat-icon [svgIcon]="'sp:' + e.icon" aria-hidden="true" /> }
            {{ e.label }}
          </button>
        }
      </div>
    </ng-template>
  `,
  styles: `
    :host {
      display: inline-flex;
    }
  `,
})
export class MenuButton {
  readonly label = input('More actions');
  readonly entries = input.required<readonly MenuEntry[]>();
  readonly small = input(true);
  readonly picked = output<string>();
}
