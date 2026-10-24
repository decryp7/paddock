import { TestBed } from '@angular/core/testing';
import { DEFAULT_STILL_WORKING_AFTER_MS, JOBS_CLOCK, JOBS_CONFIG } from './jobs-config';

describe('the jobs defaults', () => {
  afterEach(() => vi.useRealTimers());

  it('says "Still working" after 10 s', () => {
    expect(TestBed.inject(JOBS_CONFIG)).toEqual({ stillWorkingAfterMs: DEFAULT_STILL_WORKING_AFTER_MS });
    expect(DEFAULT_STILL_WORKING_AFTER_MS).toBe(10_000);
  });

  it('reads the wall clock and ticks until stopped', () => {
    vi.useFakeTimers({ now: 5_000 });
    const clock = TestBed.inject(JOBS_CLOCK);
    expect(clock.now()).toBe(5_000);
    const tick = vi.fn();
    const stop = clock.every(1_000, tick);
    vi.advanceTimersByTime(2_500);
    expect(tick).toHaveBeenCalledTimes(2);
    stop();
    vi.advanceTimersByTime(5_000);
    expect(tick).toHaveBeenCalledTimes(2);
  });
});
