import { TestBed } from '@angular/core/testing';
import { CLOCK_TICK_MS, ClockService } from './clock.service';

// The real clock back after every test, so a test that times out cannot leave fake timers for the next one (ER-12, C154).
afterEach(() => vi.useRealTimers());

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
});
