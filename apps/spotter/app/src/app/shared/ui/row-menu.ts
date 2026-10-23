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

/**
 * The entries of a menu (CDK menu: arrow keys, Esc, focus return). It is the body of both the "..." button's menu and a
 * row's right-click menu, so a row has one list of actions in both places.
 */
@Component({
  selector: 'sp-menu-panel',
  imports: [MatIcon, CdkMenu, CdkMenuItem],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="menu" cdkMenu [attr.aria-label]="label()">
      @for (e of entries(); track e.id) {
        @if (e.separated) { <div class="menu-sep" role="separator"></div> }
        <button type="button" class="menu-item" [class.danger]="e.danger" cdkMenuItem [cdkMenuItemDisabled]="!!e.disabled" (cdkMenuItemTriggered)="picked.emit(e.id)">
          @if (e.icon) { <mat-icon [svgIcon]="'sp:' + e.icon" aria-hidden="true" /> }
          {{ e.label }}
        </button>
      }
    </div>
  `,
  styles: `
    :host {
      display: contents;
    }
  `,
})
export class MenuPanel {
  readonly entries = input.required<readonly MenuEntry[]>();
  readonly label = input<string | null>(null);
  readonly picked = output<string>();
}

/** The "..." button with its menu. */
@Component({
  selector: 'sp-menu-button',
  imports: [MatIcon, CdkMenuTrigger, MenuPanel],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <button type="button" class="btn ghost icon more" [class.sm]="small()" [cdkMenuTriggerFor]="menu" [attr.aria-label]="label()" [attr.title]="label()" (click)="$event.stopPropagation()">
      <mat-icon svgIcon="sp:more" aria-hidden="true" />
    </button>
    <ng-template #menu><sp-menu-panel [entries]="entries()" [label]="label()" (picked)="picked.emit($event)" /></ng-template>
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
