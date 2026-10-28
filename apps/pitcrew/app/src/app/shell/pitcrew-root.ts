import { ChangeDetectionStrategy, Component } from '@angular/core';
import { PdShellFrame } from '@decryp7/paddock-ui/shell/pages';
import { CopyMenuHost } from '../shared/ui/copy-menu';

/** The main window: the pages frame (sidebar, status bar, menus, notifications, jobs, dialogs) from Pitcrew's contributions. */
@Component({
  selector: 'pc-root',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [PdShellFrame, CopyMenuHost],
  template: `<pd-shell-frame /><pc-copy-menu />`,
  styles: `:host { display: block; height: 100vh; }`,
})
export class PitcrewRoot {}
