import { computed, Injectable, Signal, signal } from '@angular/core';

/** One chip of the timeline's chip strip: a capture, a dump or UMDH log, or a note, anchored at `x`. */
export interface StripChip {
  /** Unique within the strip (the source plus the item's id). */
  id: string;
  /** Position of its anchor in px from the plot's left edge, and its estimated width in px. */
  x: number;
  w: number;
  /** The visible text, the tooltip and the accessible name (also the name of its entry in the overflow menu). */
  text: string;
  title: string;
  label: string;
  testid: string;
  /** A CSS colour for its border. */
  accent: string;
  /** What clicking it (or choosing its menu entry) does. */
  open(): void;
}

/**
 * The chips the timeline's tracks (captures, dumps, notes) offer to its chip strip. Each track registers its own under
 * its source name; the strip lays out all of them together. Provided by the timeline panel, one per panel.
 */
@Injectable()
export class TimelineChips {
  private readonly sources = signal<ReadonlyMap<string, Signal<readonly StripChip[]>>>(new Map());
  /** Every registered chip, by source in registration order. */
  readonly all = computed(() => [...this.sources().values()].flatMap((chips) => chips()));

  /** Registers the chips of `source`, which follow the signal. */
  register(source: string, chips: Signal<readonly StripChip[]>): void {
    this.sources.update((m) => new Map(m).set(source, chips));
  }

  /** Removes the chips of `source` (its track is gone). */
  clear(source: string): void {
    this.sources.update((m) => {
      const next = new Map(m);
      next.delete(source);
      return next;
    });
  }
}
