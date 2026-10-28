import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { MatIcon } from '@angular/material/icon';
import { SettingsStore } from '../../state/settings.store';

/** What the last settings save said, inline where the user changed something (a result of their own action, not a notice). */
@Component({
  selector: 'pc-saved-note',
  imports: [MatIcon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <p class="saved-note" role="status" [hidden]="!settings.savedNote()">
      @if (settings.savedNote(); as note) {
        <mat-icon svgIcon="pc:check-circle" aria-hidden="true" />{{ note }}
      }
    </p>
  `,
  styles: `
    :host {
      display: contents;
    }
    .saved-note[hidden] {
      display: none;
    }
    .saved-note {
      display: flex;
      align-items: center;
      gap: var(--space-2);
      margin: 0;
      color: var(--success);
      font-size: var(--text-sm);
    }
    .saved-note mat-icon {
      width: 16px;
      height: 16px;
    }
  `,
})
export class SavedNote {
  protected readonly settings = inject(SettingsStore);
}
