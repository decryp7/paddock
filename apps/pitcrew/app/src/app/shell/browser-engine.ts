import { Injectable, inject } from '@angular/core';
import { MemoryJobsBackend } from '@decryp7/paddock-ui/shell/jobs';
import { MemoryNotificationCentre, MemoryServiceHealth, type Cause, type ServiceHealthSnapshot } from '@decryp7/paddock-ui/shell/notify';
import { BACKEND } from '../core/backend';
import type { ConnectionState } from '../core/api.types';
import { SERVICE_ID } from './pitcrew.contributions';

/** The browser build's jobs (the mock backend reports its long work here, as the engine does as job events). */
export const browserJobs = new MemoryJobsBackend();

const cause = (message: string, hint: string, detail: string, code: string): Cause => ({ message, hint, detail, code, raiseCap: null, settings: null });

/** What an unreachable service means for the chip (mirrors `health::cause_of` in Rust). */
export function causeOf(c: ConnectionState, installed: boolean): Cause | null {
  if (c.state === 'unavailable' && !installed) {
    return cause(
      'The Pitcrew service is not installed, so runners are not being managed.',
      'Choose Restart to install it; Windows asks for administrator permission (a UAC prompt), macOS asks you to approve Pitcrew in System Settings.',
      c.message,
      'pc.service.not-installed',
    );
  }
  if (c.state === 'unavailable') {
    return cause('Pitcrew cannot reach its background service, so runners are not being managed.', 'Choose Restart to register and start the service again.', c.message, 'pc.service.unavailable');
  }
  if (c.state === 'mismatch') {
    return cause('Pitcrew and its background service are different versions.', 'Choose Restart to start the service that belongs to this version; if it stays, reinstall Pitcrew.', c.message, 'pc.service.mismatch');
  }
  return null;
}

/** The stand-in's Repair does what Rust's does: it installs and starts the service (here the mock's), then settles. */
class BrowserServiceHealth extends MemoryServiceHealth {
  constructor(
    centre: MemoryNotificationCentre,
    private readonly installService: () => Promise<void>,
  ) {
    super([{ id: SERVICE_ID, label: 'the Pitcrew service' }], { centre, autoSettle: true, repairOutcome: () => 'ok' });
  }

  override async repair(service: string): Promise<ServiceHealthSnapshot> {
    const snapshot = await super.repair(service);
    await this.installService().catch(() => undefined);
    return snapshot;
  }
}

/**
 * The browser build's stand-in for `paddock-health`: the service as a health service with a Repair, the notification centre,
 * both fed by the mock backend's connection exactly as Rust's event thread feeds the real ones. The shell's notify code is the
 * same as in the app; only the backends are in memory.
 */
@Injectable({ providedIn: 'root' })
export class BrowserHealth {
  private readonly backend = inject(BACKEND);
  readonly centre = new MemoryNotificationCentre();
  readonly health = new BrowserServiceHealth(this.centre, () => this.backend.installService());

  constructor() {
    this.backend.watch((e) => {
      if (e.kind !== 'connection') return;
      const connection = e.connection;
      // Whether the OS has the service registered is asked only for a lost one, as Rust's event thread does.
      const installed = connection.state === 'unavailable' ? this.backend.serviceInstall().then((s) => s.installed, () => true) : Promise.resolve(true);
      void installed.then((isInstalled) => {
        const cause = causeOf(connection, isInstalled);
        if (cause) this.health.fail(SERVICE_ID, cause, 'restart');
        else if (connection.state === 'connected') this.health.ok(SERVICE_ID);
      });
    });
  }
}
