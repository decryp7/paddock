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
    <div class="sd-toast"><pd-undo-toast /></div>
  `,
  styles: `
    :host { display: block; }
    .sd-toast { position: fixed; inset-inline: var(--pd-space-4); bottom: calc(var(--pd-space-4) + 28px); z-index: 20; display: flex; justify-content: center; pointer-events: none; }
    .sd-toast > * { pointer-events: auto; max-width: 100%; }
  `,
})
export class ShellDemo {}
