import { Injectable, inject } from '@angular/core';
import { MemoryJobsBackend } from '@decryp7/paddock-ui/shell/jobs';
import { MemoryNotificationCentre, MemoryServiceHealth, type Cause } from '@decryp7/paddock-ui/shell/notify';
import { BACKEND } from '../core/backend';
import type { ConnectionState } from '../core/api.types';
import { SERVICE_ID } from './pitcrew.contributions';

/** The browser build's jobs (the mock backend reports its long work here, as the engine does over `paddock://jobs`). */
export const browserJobs = new MemoryJobsBackend();

const cause = (message: string, hint: string, detail: string, code: string): Cause => ({ message, hint, detail, code, raiseCap: null, settings: null });

/** What an unreachable service means for the chip (mirrors `health::cause_of` in Rust). */
export function causeOf(c: ConnectionState): Cause | null {
  if (c.state === 'unavailable') {
    return cause('Pitcrew cannot reach its background service, so runners are not being managed.', 'Choose Repair to register and start the service again.', c.message, 'pc.service.unavailable');
  }
  if (c.state === 'mismatch') {
    return cause('Pitcrew and its background service are different versions.', 'Choose Repair to start the service that belongs to this version; if it stays, reinstall Pitcrew.', c.message, 'pc.service.mismatch');
  }
  return null;
}

/**
 * The browser build's stand-in for `paddock-health`: the service as a health service with a Repair, the notification centre,
 * both fed by the mock backend's connection exactly as Rust's event thread feeds the real ones. The shell's notify code is the
 * same as in the app; only the backends are in memory.
 */
@Injectable({ providedIn: 'root' })
export class BrowserHealth {
  readonly centre = new MemoryNotificationCentre();
  readonly health = new MemoryServiceHealth([{ id: SERVICE_ID, label: 'the Pitcrew service' }], { centre: this.centre, autoSettle: true, repairOutcome: () => 'ok' });

  constructor() {
    const backend = inject(BACKEND);
    backend.watch((e) => {
      if (e.kind !== 'connection') return;
      const cause = causeOf(e.connection);
      if (cause) this.health.fail(SERVICE_ID, cause, 'restart');
      else if (e.connection.state === 'connected') this.health.ok(SERVICE_ID);
    });
  }
}
