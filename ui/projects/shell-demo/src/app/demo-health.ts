import { computed, inject, Injectable, signal } from '@angular/core';
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
      { id: 'sd.agent', label: 'the demo agent' },
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
      this.health.fail('sd.engine', cause('The demo engine stopped unexpectedly.', 'Restarting it brings the engine data back.', 'demo-engine exited (signal: 6)'));
    }
  }

  breakIndex(): void {
    this.health.fail(
      'sd.index',
      cause('The search index stopped: a segment is damaged.', 'Search is unavailable until it is reset; resetting rebuilds it from the logs.', 'index: checksum mismatch in segment 3'),
      'reset',
    );
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
  /**
   * The panel cannot show engine data: its last load met an outage, or the engine is down now (the desktop app hears
   * that from Rust after the load). Either way the panel waits calmly with the chip's cause instead of showing stale data.
   */
  readonly failed = computed(() => this.lastFailed() || (this.health ? !this.health.isServing('sd.engine') : false));

  load(): void {
    this.count.update((n) => n + 1);
    this.lastFailed.set(this.health ? !this.health.isServing('sd.engine') : false);
  }
}
