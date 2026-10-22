import { TestBed } from '@angular/core/testing';
import { ClockService } from '../../core/clock.service';
import { AgoPipe } from './ago.pipe';

describe('AgoPipe', () => {
  it('formats relative to the clock signal and follows it', () => {
    const clock = TestBed.inject(ClockService);
    clock.set(1_000_000_000);
    const pipe = TestBed.runInInjectionContext(() => new AgoPipe());
    expect(pipe.transform(1_000_000_000 - 5 * 60_000)).toBe('5 min ago');
    expect(pipe.transform(1_000_000_000 + 2 * 3_600_000)).toBe('in 2 h');
    expect(pipe.transform(null)).toBe('—');
    expect(pipe.transform(undefined)).toBe('—');
    clock.set(1_000_000_000 + 10 * 60_000);
    expect(pipe.transform(1_000_000_000 - 5 * 60_000)).toBe('15 min ago');
  });
});
