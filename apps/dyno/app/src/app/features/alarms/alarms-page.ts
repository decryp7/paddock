import { ChangeDetectionStrategy, Component } from '@angular/core';

/** Alarms with custom rules (a stub the page's owner replaces). */
@Component({
  selector: 'dy-alarms-page',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `<div class="page"><header class="page-head"><div class="titles"><h1>Alarms</h1><p>What needs attention.</p></div></header></div>`,
})
export class AlarmsPage {}
