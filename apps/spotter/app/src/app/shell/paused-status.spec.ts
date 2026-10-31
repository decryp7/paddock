import { isPaused, pausedText } from './paused-status';

describe('the paused notifications status', () => {
  it('says how long notifications stay paused', () => {
    expect(pausedText(1_000_000 + 42 * 60_000, 1_000_000)).toBe('Notifications paused for 42 min');
  });

  it('never says zero: the last minute rounds up', () => {
    expect(pausedText(1_000_000 + 5_000, 1_000_000)).toBe('Notifications paused for 1 min');
  });
});

describe('isPaused', () => {
  it('is true only before the end of the pause', () => {
    expect(isPaused(2000, 1000)).toBe(true);
    expect(isPaused(1000, 1000)).toBe(false);
    expect(isPaused(null, 1000)).toBe(false);
    expect(isPaused(undefined, 1000)).toBe(false);
  });
});
