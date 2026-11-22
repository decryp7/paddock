import { describe, expect, it, vi } from 'vitest';
import { until } from './until';

describe('until', () => {
  it('returns at once when the condition already holds, without a round', async () => {
    const settle = vi.fn(async () => undefined);
    await until(() => true, { what: 'ready', settle });
    expect(settle).not.toHaveBeenCalled();
  });

  it('runs the settle round between reads and returns as soon as the condition holds', async () => {
    let n = 0;
    const settle = vi.fn(async () => void n++);
    await until(() => n >= 3, { what: 'three rounds', settle });
    expect(settle).toHaveBeenCalledTimes(3);
  });

  it('treats a throw like a false, so an expect inside works, and a void return as holding', async () => {
    let n = 0;
    await until(
      () => {
        expect(++n).toBeGreaterThan(2);
      },
      { what: 'an expect that passes the third time', settle: async () => undefined },
    );
    expect(n).toBe(3);
  });

  it('fails naming what it waited for, with the last error as the cause (positive control: a condition that never holds)', async () => {
    const failure = until(
      () => {
        throw new Error('still empty');
      },
      { what: 'the result list to fill', maxRounds: 4, settle: async () => undefined },
    );
    await expect(failure).rejects.toThrow('the result list to fill did not happen within 4 rounds');
    await expect(failure).rejects.toHaveProperty('cause.message', 'still empty');
  });

  it('fails a condition that stays false, and does not wait for real time to pass (a fixed number of rounds)', async () => {
    const settle = vi.fn(async () => undefined);
    await expect(until(() => false, { what: 'never', maxRounds: 50, settle })).rejects.toThrow('never did not happen within 50 rounds');
    expect(settle).toHaveBeenCalledTimes(50);
  });

  it('uses one event-loop turn by default so a timer-driven condition gets there', async () => {
    let done = false;
    setTimeout(() => (done = true), 5);
    await until(() => done, { what: 'a 5 ms timer' });
    expect(done).toBe(true);
  });

  it('refuses a wait with no description', async () => {
    await expect(until(() => true, { what: '' })).rejects.toThrow('say what the wait is for');
  });
});
