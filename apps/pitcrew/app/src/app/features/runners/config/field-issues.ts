import { ChangeDetectionStrategy, Component, input } from '@angular/core';
import { MatIcon } from '@angular/material/icon';
import type { ConfigIssue } from '../../../core/api.types';

/** The engine's findings about one field: an icon and the word (error / warning), then its message. */
@Component({
  selector: 'pc-field-issues',
  imports: [MatIcon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @for (i of issues(); track $index) {
      <p class="issue" [class.error]="i.severity === 'error'" [attr.id]="prefix() + '-' + $index">
        <mat-icon [svgIcon]="i.severity === 'error' ? 'pc:x-octagon' : 'pc:alert'" aria-hidden="true" />
        <span><strong>{{ i.severity === 'error' ? 'Error' : 'Warning' }}:</strong> {{ i.message }}</span>
      </p>
    }
  `,
  styles: `
    :host {
      display: contents;
    }
    .issue {
      display: flex;
      gap: var(--space-1);
      align-items: flex-start;
      margin: 0;
      font-size: var(--text-sm);
      color: var(--warning);
    }
    .issue.error {
      color: var(--danger);
    }
    mat-icon {
      flex: none;
      width: 14px;
      height: 14px;
      margin-top: 2px;
    }
  `,
})
export class FieldIssues {
  readonly issues = input.required<readonly ConfigIssue[]>();
  /** Prefix for the paragraph ids (for `aria-describedby`). */
  readonly prefix = input('issue');
}
