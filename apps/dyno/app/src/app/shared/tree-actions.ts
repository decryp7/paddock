import { ChangeDetectionStrategy, Component, input, output } from '@angular/core';
import { MatIconButton } from '@angular/material/button';
import { MatTooltip } from '@angular/material/tooltip';
import { Icon } from './icon';

/**
 * "Expand all" and "Collapse all" for a tree or grouped list: the same two icon buttons in every toolbar. Interaction state
 * only: the page opens or folds its own rows when it hears the outputs. `what` names the things folded for the accessible
 * name ("Expand all programs").
 */
@Component({
  selector: 'dy-tree-actions',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [MatIconButton, MatTooltip, Icon],
  template: `
    <span class="tree-actions" role="group" [attr.aria-label]="'Expand or collapse ' + what()">
      <button type="button" matIconButton data-testid="tree-expand-all" [disabled]="disabled()" [attr.aria-label]="'Expand all ' + what()" [matTooltip]="'Expand all ' + what()" (click)="expandAll.emit()">
        <dy-icon name="expand-all" />
      </button>
      <button type="button" matIconButton data-testid="tree-collapse-all" [disabled]="disabled()" [attr.aria-label]="'Collapse all ' + what()" [matTooltip]="'Collapse all ' + what()" (click)="collapseAll.emit()">
        <dy-icon name="collapse-all" />
      </button>
    </span>
  `,
  styles: `
    :host { display: inline-flex; flex: none; }
    .tree-actions { display: inline-flex; align-items: center; }
  `,
})
export class TreeActions {
  /** What the tree holds, in lower case and plural ("programs", "groups"). */
  readonly what = input('groups');
  readonly disabled = input(false);
  readonly expandAll = output<void>();
  readonly collapseAll = output<void>();
}
