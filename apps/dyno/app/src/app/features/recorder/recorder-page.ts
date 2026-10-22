import { ChangeDetectionStrategy, Component } from '@angular/core';

/** The flight recorder timeline (a stub the page's owner replaces). */
@Component({
  selector: 'dy-recorder-page',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `<div class="page"><header class="page-head"><div class="titles"><h1>Flight recorder</h1><p>The last ten minutes before something went wrong.</p></div></header></div>`,
})
export class RecorderPage {}
