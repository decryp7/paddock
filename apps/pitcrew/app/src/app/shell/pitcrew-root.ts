import { ChangeDetectionStrategy, Component } from '@angular/core';
import { PdShellFrame } from '@decryp7/paddock-ui/shell/pages';

/** The main window: the pages frame (sidebar, status bar, menus, notifications, jobs, dialogs, the Copy menu) from Pitcrew's contributions. */
@Component({
  selector: 'pc-root',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [PdShellFrame],
  template: `<pd-shell-frame />`,
  styles: `:host { display: block; height: 100vh; }`,
})
export class PitcrewRoot {}
