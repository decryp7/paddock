import { ChangeDetectionStrategy, Component } from '@angular/core';
import { DialogFrame } from '../shared/dialog';
import { Icon } from '../shared/icon';
import { COMING } from './nav';

/** What is on the roadmap without a screen yet: one neutral list, so no page exists for a feature that does not. */
@Component({
  selector: 'dy-coming-dialog',
  imports: [DialogFrame, Icon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <dy-dialog title="Coming" subtitle="Not built yet. Dyno only looks." icon="info">
      <ul>
        @for (c of coming; track c.label) {
          <li>
            <dy-icon [name]="c.icon" />
            <div><strong>{{ c.label }}</strong><p>{{ c.text }}</p></div>
          </li>
        }
      </ul>
    </dy-dialog>
  `,
  styles: `
    ul { list-style: none; margin: 0; padding: 0; display: grid; }
    li { display: flex; gap: var(--sp-3); padding: var(--sp-3) 0; border-top: 1px solid var(--border-subtle); }
    li:first-child { border-top: 0; }
    li dy-icon { --icon-size: 18px; color: var(--text-3); margin-top: 1px; }
    p { margin: 2px 0 0; color: var(--text-2); font-size: var(--fs-sm); }
  `,
})
export class ComingDialog {
  protected readonly coming = COMING;
}
