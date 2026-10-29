import { ChangeDetectionStrategy, Component } from '@angular/core';
import { PdShellFrame } from '@decryp7/paddock-ui/shell/pages';
import { PdUndoToast } from '@decryp7/paddock-ui/shell/undo';

/**
 * The reference app of the Paddock app shell (ADR 0012 §7) on the pages frame, against the mock backend. Its whole
 * window is the frame: the pages, status items, menus and dialogs come from `demo.contributions.ts`. Each work package
 * of the plan adds its part here, with its e2e.
 */
@Component({
  selector: 'sd-root',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [PdShellFrame, PdUndoToast],
  template: `
    <pd-shell-frame />
    <pd-undo-toast />
  `,
  styles: `
    :host { display: block; }
  `,
})
export class ShellDemo {}
