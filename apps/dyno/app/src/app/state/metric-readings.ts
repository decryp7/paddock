import { DestroyRef, Injectable, effect, inject, signal, untracked, type Signal } from '@angular/core';
import { BACKEND } from '../core/backend';
import type { MetricReading, ReadingQuery } from '../core/generated/widgets';
import { LiveStore } from './live.store';

/**
 * Readings of metrics with their tones, from the engine's one reader (family ADR 0035): the engine turns the newest live
 * update into the number, how it is said, its place on a 0 to 100 scale and its tone against the widget's thresholds.
 * `follow` asks again on every live tick, one request at a time; the UI draws what comes back.
 */
@Injectable({ providedIn: 'root' })
export class MetricReadings {
  private readonly backend = inject(BACKEND);
  private readonly live = inject(LiveStore);
  private readonly destroy = inject(DestroyRef);

  /**
   * Follows `queries`: the answers, in the order of the queries (empty before the first one). Call in an injection context;
   * it stops with the caller.
   */
  follow(queries: () => readonly ReadingQuery[]): Signal<readonly MetricReading[]> {
    const out = signal<readonly MetricReading[]>([]);
    let busy = false;
    let stopped = false;
    this.destroy.onDestroy(() => (stopped = true));
    const ask = async (list: readonly ReadingQuery[]): Promise<void> => {
      if (busy) return;
      busy = true;
      try {
        const answer = list.length === 0 ? [] : await this.backend.metricReadings([...list]);
        if (!stopped) out.set(answer);
      } catch {
        // The cards keep their last readings.
      } finally {
        busy = false;
      }
    };
    effect(() => {
      const list = queries();
      this.live.update();
      void untracked(() => ask(list));
    });
    return out.asReadonly();
  }
}
