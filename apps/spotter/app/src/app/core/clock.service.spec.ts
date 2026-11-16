import { TestBed } from '@angular/core/testing';
import { holdableTimers, withLiveHeld } from '@decryp7/paddock-ui/testing';
import { CLOCK_TICK_MS, CLOCK_TIMERS, ClockService } from './clock.service';

describe('ClockService', () => {
  it('ticks and can be pinned', () => {
    vi.useFakeTimers();
    vi.setSystemTime(1_000_000);
    const c = TestBed.inject(ClockService);
    c.set(5);
    expect(c.now()).toBe(5);
    vi.advanceTimersByTime(CLOCK_TICK_MS);
    expect(c.now()).toBe(1_000_000 + CLOCK_TICK_MS);
    TestBed.resetTestingModule();
    vi.useRealTimers();
  });

  it('the Today clock does not move under a read while the live clock is held, and moves once released', async () => {
    vi.useFakeTimers();
    vi.setSystemTime(1_000_000);
    const timers = holdableTimers({
      setTimeout: (fn: () => void, ms: number) => setTimeout(fn, ms),
      clearTimeout: (h: unknown) => clearTimeout(h as never),
      setInterval: (fn: () => void, ms: number) => setInterval(fn, ms),
      clearInterval: (h: unknown) => clearInterval(h as never),
    });
    TestBed.configureTestingModule({ providers: [{ provide: CLOCK_TIMERS, useValue: timers }] });
    const c = TestBed.inject(ClockService);
    await withLiveHeld(async () => {
      const read = c.now();
      vi.advanceTimersByTime(CLOCK_TICK_MS * 3);
      expect(c.now()).toBe(read);
    });
    expect(c.now()).toBe(1_000_000 + CLOCK_TICK_MS * 3);
    TestBed.resetTestingModule();
    vi.useRealTimers();
  });
});
