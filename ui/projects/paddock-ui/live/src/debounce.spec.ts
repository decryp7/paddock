import { Debouncer } from './debounce';

/** Timers moved by hand. */
function fakeTimers() {
  let next = 1;
  const due = new Map<number, { at: number; fn: () => void }>();
  let now = 0;
  return {
    timers: {
      setTimeout: (fn: () => void, ms: number) => {
        due.set(next, { at: now + ms, fn });
        return next++;
      },
      clearTimeout: (h: unknown) => void due.delete(h as number),
    },
    advance(ms: number) {
      now += ms;
      for (const [h, t] of [...due]) {
        if (t.at <= now) {
          due.delete(h);
          t.fn();
        }
      }
    },
    get waiting() {
      return due.size;
    },
  };
}

describe('Debouncer', () => {
  it('runs once, a wait after the last trigger of a burst', () => {
    const t = fakeTimers();
    const run = vi.fn();
    const d = new Debouncer(400, run, t.timers);
    d.trigger();
    t.advance(300);
    d.trigger();
    t.advance(300);
    expect(run).not.toHaveBeenCalled();
    expect(d.pending).toBe(true);
    t.advance(100);
    expect(run).toHaveBeenCalledTimes(1);
    expect(d.pending).toBe(false);
    expect(t.waiting).toBe(0);
  });

  it('flush runs a waiting run now and does nothing otherwise', () => {
    const t = fakeTimers();
    const run = vi.fn();
    const d = new Debouncer(400, run, t.timers);
    d.flush();
    expect(run).not.toHaveBeenCalled();
    d.trigger();
    d.flush();
    expect(run).toHaveBeenCalledTimes(1);
    t.advance(1000);
    expect(run).toHaveBeenCalledTimes(1);
  });

  it('cancel drops a waiting run', () => {
    const t = fakeTimers();
    const run = vi.fn();
    const d = new Debouncer(400, run, t.timers);
    d.trigger();
    d.cancel();
    d.cancel();
    t.advance(1000);
    expect(run).not.toHaveBeenCalled();
  });

  it('uses the global timers by default', () => {
    vi.useFakeTimers();
    try {
      const run = vi.fn();
      const d = new Debouncer(50, run);
      d.trigger();
      d.trigger();
      vi.advanceTimersByTime(50);
      expect(run).toHaveBeenCalledTimes(1);
    } finally {
      vi.useRealTimers();
    }
  });
});
