import { InjectionToken } from '@angular/core';

/**
 * The animation-frame clock of the grid's gestures (rule 60: tests control time). The app uses `requestAnimationFrame`; a
 * spec provides a stepped fake and runs each queued callback itself.
 */
export interface FrameScheduler {
  /** Runs `callback` once on the next frame; returns a handle for `cancel`. */
  request(callback: () => void): number;
  /** Drops a request that has not run yet. */
  cancel(handle: number): void;
}

/** The frame scheduler of `pdGridGesture` and `pd-widget-grid`; `requestAnimationFrame` unless the app provides another. */
export const PD_FRAME_SCHEDULER = new InjectionToken<FrameScheduler>('PD_FRAME_SCHEDULER', {
  providedIn: 'root',
  factory: () => ({
    request: (callback) => requestAnimationFrame(() => callback()),
    cancel: (handle) => cancelAnimationFrame(handle),
  }),
});

/**
 * A frame scheduler the test steps by hand (rule 60: tests control time): `request` queues, `flush` runs what is queued now
 * (callbacks queued while running wait for the next `flush`). Provide it for `PD_FRAME_SCHEDULER` in a spec.
 */
export class ManualFrameScheduler implements FrameScheduler {
  private next = 0;
  /** The callbacks waiting for a frame. */
  readonly queue = new Map<number, () => void>();

  request(callback: () => void): number {
    this.queue.set(++this.next, callback);
    return this.next;
  }

  cancel(handle: number): void {
    this.queue.delete(handle);
  }

  /** Runs one frame. */
  flush(): void {
    const run = [...this.queue.values()];
    this.queue.clear();
    run.forEach((f) => f());
  }
}
