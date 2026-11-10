import { MockClock, MockTimer } from '../app/core/mock/mock-clock';

/** A clock that only moves when told: every mock timer becomes a step the test takes. */
export class ManualClock implements MockClock {
  private t = 1_000_000;
  private seq = 0;
  private readonly timers = new Map<number, { at: number; fn: () => void; every: number | null }>();

  now = () => this.t;
  elapsed = () => this.t;
  setTimeout = (fn: () => void, ms: number) => this.add(fn, ms, null);
  setInterval = (fn: () => void, ms: number) => this.add(fn, ms, Math.max(1, ms));
  clearTimeout = (t: MockTimer | undefined) => void this.timers.delete(t as unknown as number);
  clearInterval = (t: MockTimer | undefined) => void this.timers.delete(t as unknown as number);
  sleep = (ms: number) => new Promise<void>((resolve) => this.setTimeout(resolve, ms));

  private add(fn: () => void, ms: number, every: number | null): MockTimer {
    const id = ++this.seq;
    this.timers.set(id, { at: this.t + Math.max(0, ms), fn, every });
    return id as unknown as MockTimer;
  }

  /** Moves time on by `ms`, firing every timer due on the way (and letting the promises they settle run). */
  async advance(ms: number): Promise<void> {
    const end = this.t + ms;
    for (;;) {
      const next = [...this.timers.entries()].filter(([, t]) => t.at <= end).sort((a, b) => a[1].at - b[1].at)[0];
      if (!next) break;
      const [id, timer] = next;
      this.t = timer.at;
      if (timer.every === null) this.timers.delete(id);
      else timer.at += timer.every;
      timer.fn();
      for (let i = 0; i < 20; i++) await Promise.resolve();
    }
    this.t = end;
  }
}
