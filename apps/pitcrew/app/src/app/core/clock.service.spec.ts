import { TestBed } from '@angular/core/testing';
import { ClockService } from './clock.service';

function setVisibility(state: 'visible' | 'hidden'): void {
  Object.defineProperty(document, 'visibilityState', { value: state, configurable: true });
  document.dispatchEvent(new Event('visibilitychange'));
}

describe('ClockService', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(1_000_000);
    TestBed.resetTestingModule();
  });
  afterEach(() => {
    setVisibility('visible');
    vi.useRealTimers();
  });

  it('ticks once a second while visible', () => {
    const clock = TestBed.inject(ClockService);
    expect(clock.visible()).toBe(true);
    expect(clock.now()).toBe(1_000_000);
    vi.advanceTimersByTime(3000);
    expect(clock.now()).toBe(1_003_000);
  });

  it('pauses while hidden and resumes with a fresh time when shown', () => {
    const clock = TestBed.inject(ClockService);
    setVisibility('hidden');
    expect(clock.visible()).toBe(false);
    vi.advanceTimersByTime(5000);
    expect(clock.now()).toBe(1_000_000);
    setVisibility('visible');
    expect(clock.visible()).toBe(true);
    expect(clock.now()).toBe(1_005_000);
    vi.advanceTimersByTime(1000);
    expect(clock.now()).toBe(1_006_000);
  });

  it('does not start a timer when created hidden, and stops on destroy', () => {
    Object.defineProperty(document, 'visibilityState', { value: 'hidden', configurable: true });
    const clock = TestBed.inject(ClockService);
    expect(clock.visible()).toBe(false);
    vi.advanceTimersByTime(2000);
    expect(clock.now()).toBe(1_000_000);
    setVisibility('visible');
    const shown = clock.now();
    TestBed.resetTestingModule();
    vi.advanceTimersByTime(5000);
    expect(clock.now()).toBe(shown);
  });
});
