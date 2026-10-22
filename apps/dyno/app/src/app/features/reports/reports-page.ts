import { ChangeDetectionStrategy, Component } from '@angular/core';

/** Placeholder until the screen lands. */
@Component({
  selector: 'dy-reports-page',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `<div class="page"><h1>ReportsPage</h1></div>`,
})
export class ReportsPage {}
