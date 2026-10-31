import { DestroyRef, Injectable, effect, inject, signal, untracked, type Signal } from '@angular/core';
import { BACKEND } from '../core/backend';
import type { MetricReading, ReadingQuery } from '../core/generated/widgets';

/** One thing to read, named by the caller (a widget's placement id), so an answer never lands on another widget when the list changes. */
export interface NamedQuery {
  readonly key: string;
  readonly query: ReadingQuery;
}
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
   * Follows `queries`: the answers by key (empty before the first one). Each answer is matched to the key it was asked for, from
   * the list as it was when the request was made. Call in an injection context; it stops with the caller.
   */
  follow(queries: () => readonly NamedQuery[]): Signal<ReadonlyMap<string, MetricReading>> {
    const out = signal<ReadonlyMap<string, MetricReading>>(new Map());
    let busy = false;
    let stopped = false;
    this.destroy.onDestroy(() => (stopped = true));
    const ask = async (list: readonly NamedQuery[]): Promise<void> => {
      if (busy) return;
      busy = true;
      try {
        const answer = list.length === 0 ? [] : await this.backend.metricReadings(list.map((q) => q.query));
        if (!stopped) out.set(new Map(list.flatMap((q, i) => (answer[i] ? [[q.key, answer[i]] as const] : []))));
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
