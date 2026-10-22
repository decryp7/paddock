import { ChangeDetectionStrategy, Component } from '@angular/core';

/** Placeholder until the screen lands. */
@Component({
  selector: 'dy-health-page',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `<div class="page"><h1>HealthPage</h1></div>`,
})
export class HealthPage {}
