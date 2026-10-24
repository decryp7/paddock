import type { Timers } from '@decryp7/paddock-ui/ipc';

const GLOBAL_TIMERS: Timers = {
  setTimeout: (fn, ms) => globalThis.setTimeout(fn, ms),
  clearTimeout: (handle) => globalThis.clearTimeout(handle as ReturnType<typeof setTimeout>),
};

/**
 * Collects a burst of pushes into one run (Spotter's dashboard reload): `trigger` starts or restarts the wait, and `run` is
 * called once `waitMs` after the last trigger. Timers are injectable, so a spec moves time by hand.
 */
export class Debouncer {
  private handle: unknown = null;

  constructor(
    private readonly waitMs: number,
    private readonly run: () => void,
    private readonly timers: Timers = GLOBAL_TIMERS,
  ) {}

  /** Whether a run is waiting. */
  get pending(): boolean {
    return this.handle !== null;
  }

  /** (Re)starts the wait. */
  trigger(): void {
    this.cancel();
    this.handle = this.timers.setTimeout(() => {
      this.handle = null;
      this.run();
    }, this.waitMs);
  }

  /** Runs now if a run is waiting. */
  flush(): void {
    if (this.handle === null) return;
    this.cancel();
    this.run();
  }

  /** Drops a waiting run. */
  cancel(): void {
    if (this.handle !== null) this.timers.clearTimeout(this.handle);
    this.handle = null;
  }
}
