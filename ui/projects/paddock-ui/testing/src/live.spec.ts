import { afterEach, describe, expect, it, vi } from 'vitest';
import { holdableTimers, holdLive, liveHoldCount, releaseLive, resetLive, withLiveHeld, type TimerSource } from './live';

/** A timer source a test drives by hand: `fire` runs what is due, the way a clock that moved on would. */
function handTimers() {
  const timers = new Map<number, { fn: () => void; every: boolean }>();
  let seq = 0;
  const source: TimerSource & { fire(): void; count(): number; tag: string } = {
    tag: 'inner',
    setTimeout: (fn) => {
      timers.set(++seq, { fn, every: false });
      return seq as never;
    },
    setInterval: (fn) => {
      timers.set(++seq, { fn, every: true });
      return seq as never;
    },
    clearTimeout: (t) => void timers.delete(t as never),
    clearInterval: (t) => void timers.delete(t as never),
    fire() {
      for (const [id, t] of [...timers]) {
        if (!t.every) timers.delete(id);
        t.fn();
      }
    },
    count: () => timers.size,
  };
  return source;
}

afterEach(() => resetLive());

describe('holdableTimers', () => {
  it('runs a timer at once while not held', () => {
    const inner = handTimers();
    const held = holdableTimers(inner);
    const fn = vi.fn();
    held.setInterval(fn, 10);
    inner.fire();
    inner.fire();
    expect(fn).toHaveBeenCalledTimes(2);
  });

  it('keeps a due interval from running while held, and runs it once (coalesced) on release', () => {
    const inner = handTimers();
    const held = holdableTimers(inner);
    const fn = vi.fn();
    held.setInterval(fn, 10);
    held.hold();
    inner.fire();
    inner.fire();
    inner.fire();
    expect(fn).not.toHaveBeenCalled();
    held.release();
    expect(fn).toHaveBeenCalledTimes(1);
    inner.fire();
    expect(fn).toHaveBeenCalledTimes(2);
  });

  it('does not lose a one-shot timer that came due while held', () => {
    const inner = handTimers();
    const held = holdableTimers(inner);
    const fn = vi.fn();
    held.hold();
    held.setTimeout(fn, 5);
    inner.fire();
    expect(fn).not.toHaveBeenCalled();
    held.release();
    expect(fn).toHaveBeenCalledTimes(1);
  });

  it('drops a held tick of a timer cleared meanwhile', () => {
    const inner = handTimers();
    const held = holdableTimers(inner);
    const fn = vi.fn();
    const t = held.setInterval(fn, 10);
    held.hold();
    inner.fire();
    held.clearInterval(t);
    held.release();
    expect(fn).not.toHaveBeenCalled();
    expect(inner.count()).toBe(0);
  });

  it('keeps the rest of the wrapped source (its other members and prototype)', () => {
    class Manual {
      n = 0;
      setTimeout = (_fn: () => void, _ms: number) => 1 as never;
      setInterval = (_fn: () => void, _ms: number) => 1 as never;
      clearTimeout = (_t: unknown) => undefined;
      clearInterval = (_t: unknown) => undefined;
      advance(): number {
        return ++this.n;
      }
    }
    const held = holdableTimers(new Manual());
    expect(held.advance()).toBe(1);
  });
});

describe('holdLive, releaseLive, withLiveHeld', () => {
  it('holds every source made so far, and one made while held', () => {
    const a = holdableTimers(handTimers());
    holdLive();
    const b = holdableTimers(handTimers());
    expect(a.held).toBe(true);
    expect(b.held).toBe(true);
    expect(liveHoldCount()).toBe(1);
    releaseLive();
    expect(a.held).toBe(false);
    expect(b.held).toBe(false);
  });

  it('nests: the feed runs again only when every hold is released', () => {
    const a = holdableTimers(handTimers());
    holdLive();
    holdLive();
    releaseLive();
    expect(a.held).toBe(true);
    releaseLive();
    expect(a.held).toBe(false);
  });

  it('withLiveHeld holds around an async body, returns its value and releases when it throws', async () => {
    const inner = handTimers();
    const a = holdableTimers(inner);
    const fn = vi.fn();
    a.setInterval(fn, 1);
    const value = await withLiveHeld(async () => {
      inner.fire();
      expect(fn).not.toHaveBeenCalled();
      return 7;
    });
    expect(value).toBe(7);
    expect(fn).toHaveBeenCalledTimes(1);
    await expect(withLiveHeld(() => Promise.reject(new Error('x')))).rejects.toThrow('x');
    expect(a.held).toBe(false);
  });

  it('refuses more releases than holds, naming the fix', () => {
    expect(() => releaseLive()).toThrow(/releaseLive\(\) without a holdLive\(\)/);
  });

  it('resetLive (every test) forgets the sources and the holds', () => {
    const a = holdableTimers(handTimers());
    holdLive();
    resetLive();
    expect(liveHoldCount()).toBe(0);
    expect(holdableTimers(handTimers()).held).toBe(false);
    a.release();
  });
});
