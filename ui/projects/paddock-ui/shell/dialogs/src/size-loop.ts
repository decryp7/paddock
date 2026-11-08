import type { DialogMeasure } from './io';

/** The key a measure is compared by: equal keys are the same measure (nothing to send). */
const keyOf = (m: DialogMeasure): string => `${m.content.width}x${m.content.height}/${m.minContent.width}x${m.minContent.height}`;

/** What starts a run: a trigger resets the loop guard, a follow-up (after Rust re-laid the page out) does not. */
export type SizeRun = 'trigger' | 'follow-up';

/**
 * The one measure loop of a dialog window (ADR 0043 §2): every size change goes through `run`, which measures with the
 * frame's own function, and sends the measure only when it differs from the last one **sent** (never one read later:
 * the `08ad47b22` lesson), numbered `(epoch, seq)`.
 *
 * One change of the content produces at most two sends: the measure of the change (a `trigger`) and at most one
 * `follow-up` after Rust resized the window and the page re-laid out (a rounding of the new size can move a line). A
 * second follow-up that would send again is a loop: it is not sent, a warning with the last three measures is logged, and
 * the loop waits for the next trigger.
 */
export class SizeLoop {
  private lastKey: string | null = null;
  private followUps = 0;
  private readonly recent: string[] = [];
  /** The `seq` of the last measure sent (0: none yet). */
  seq = 0;
  /** True once a loop was stopped, until the next trigger. */
  stopped = false;

  constructor(
    /** The page load this loop sends for (`DialogStart.epoch`). */
    private readonly epoch: number,
    /** Measures the frame now, synchronously. */
    private readonly measure: () => DialogMeasure,
    /** Sends `dialog_measure`. */
    private readonly send: (epoch: number, seq: number, measure: DialogMeasure) => Promise<void>,
    /** A line for the app's log. */
    private readonly warn: (message: string) => void = (m) => console.warn(m),
    /** Told after every send (`data-fit-seq`). */
    private readonly sent: (seq: number, measure: DialogMeasure) => void = () => undefined,
  ) {}

  /** Measures now and sends when it differs from the last sent; returns the `seq` sent, or null. */
  run(kind: SizeRun = 'trigger'): number | null {
    if (kind === 'trigger') {
      this.followUps = 0;
      this.stopped = false;
    }
    const m = this.measure();
    const key = keyOf(m);
    if (key === this.lastKey) return null;
    if (kind === 'follow-up') {
      if (this.followUps >= 1) {
        this.stopped = true;
        this.warn(`dialog size loop did not settle: last measures ${[...this.recent, key].slice(-3).join(' | ')}`);
        return null;
      }
      this.followUps++;
    }
    this.lastKey = key;
    this.recent.push(key);
    if (this.recent.length > 3) this.recent.shift();
    const seq = ++this.seq;
    this.sent(seq, m);
    void this.send(this.epoch, seq, m).catch(() => undefined);
    return seq;
  }
}
