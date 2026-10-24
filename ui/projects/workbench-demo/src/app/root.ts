import { ChangeDetectionStrategy, Component } from '@angular/core';
import { PdDock } from '@decryp7/paddock-ui/workbench/dock';
import { PdWorkbench } from '@decryp7/paddock-ui/workbench/frame';

/** The demo's main window: the workbench frame with the dock as its stage. */
@Component({
  selector: 'wd-root',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [PdWorkbench, PdDock],
  template: `<pd-workbench><pd-dock slot="stage" /></pd-workbench>`,
  styles: `:host { display: block; height: 100%; }`,
})
export class WorkbenchDemo {}
