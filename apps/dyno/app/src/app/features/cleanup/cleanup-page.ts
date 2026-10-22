import { ChangeDetectionStrategy, Component } from '@angular/core';
import { Planned } from '../planned/planned';

/** Placeholder until the screen is built (see {@link Planned}); the feature replaces this component and keeps the route. */
@Component({
  selector: 'dy-cleanup-page',
  imports: [Planned],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `<dy-planned page="cleanup" />`,
})
export class CleanupPage {}
