import { ChangeDetectionStrategy, Component } from '@angular/core';
import { ServiceStatusChip } from '../../health/service-status-chip';

/** Status item (end): the service-health chip, which keeps its size when the bar is narrow. */
@Component({
  selector: 'debrief-status-service',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [ServiceStatusChip],
  template: `<debrief-service-status-chip class="keep" />`,
  styles: `
    :host { display: contents; }
    .keep { flex: none; }
  `,
})
export class StatusService {}
