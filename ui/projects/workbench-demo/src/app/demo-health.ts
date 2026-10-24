import { Injectable } from '@angular/core';
import { MemoryNotificationCentre, MemoryServiceHealth } from '@decryp7/paddock-ui/shell/notify';

/** The demo's service health (the engine's part, simulated): one service, the index, that is up. */
@Injectable({ providedIn: 'root' })
export class DemoHealth {
  readonly centre = new MemoryNotificationCentre();
  readonly health = new MemoryServiceHealth([{ id: 'wd.index', label: 'the index' }], { centre: this.centre });
}
