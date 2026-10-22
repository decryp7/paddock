import { ChangeDetectionStrategy, Component } from '@angular/core';
import { Planned } from '../planned/planned';

/** Placeholder until the screen is built (see {@link Planned}); the feature replaces this component and keeps the route. */
@Component({
  selector: 'dy-reports-page',
  imports: [Planned],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `<dy-planned page="reports" />`,
})
export class ReportsPage {}
