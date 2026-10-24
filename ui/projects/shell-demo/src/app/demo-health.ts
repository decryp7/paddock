import { inject, Injectable, signal } from '@angular/core';
import { MemoryNotificationCentre, MemoryServiceHealth, ServiceHealthStore, type Cause } from '@decryp7/paddock-ui/shell/notify';

const cause = (message: string, hint: string, detail: string): Cause => ({ message, hint, detail, code: null, raiseCap: null, settings: null });

/**
 * The demo's stand-in for `paddock-health` (the browser build has no Rust): two services, one whose Repair works (the
 * demo engine restarts) and one whose Repair fails (the search index cannot be rebuilt), and the notification centre.
 * The shell's notify code is the same as in a real app; only the backends are in memory.
 */
@Injectable({ providedIn: 'root' })
export class DemoHealthEngine {
  readonly centre = new MemoryNotificationCentre();
  readonly health = new MemoryServiceHealth(
    [
      { id: 'sd.engine', label: 'the demo engine' },
      { id: 'sd.index', label: 'the search index' },
    ],
    {
      centre: this.centre,
      autoSettle: true,
      repairOutcome: (service) =>
        service === 'sd.index'
          ? { cause: cause('The search index could not be rebuilt.', 'Free some disk space, then try again.', 'rebuild: no space left on device (os error 28)') }
          : 'ok',
    },
  );

  /** Three commands meet the same failure of the demo engine. */
  breakEngine(): void {
    for (let i = 0; i < 3; i++) {
      this.health.fail('sd.engine', cause('The demo engine stopped unexpectedly.', 'Choose Restart.', 'demo-engine exited (signal: 6)'));
    }
  }

  breakIndex(): void {
    this.health.fail('sd.index', cause('The search index is damaged.', 'Choose Try again to rebuild it.', 'index: checksum mismatch in segment 3'), 'retry');
  }
}

/**
 * A panel's data from the demo engine: it fails while the engine is down and loads again when it is back. Without the
 * window's notify part (a unit test of the commands) the engine counts as serving.
 */
@Injectable({ providedIn: 'root' })
export class DemoEngineData {
  private readonly health = inject(ServiceHealthStore, { optional: true });
  private readonly count = signal(0);
  private readonly lastFailed = signal(false);
  readonly loads = this.count.asReadonly();
  readonly failed = this.lastFailed.asReadonly();

  load(): void {
    this.count.update((n) => n + 1);
    this.lastFailed.set(this.health ? !this.health.isServing('sd.engine') : false);
  }
}
