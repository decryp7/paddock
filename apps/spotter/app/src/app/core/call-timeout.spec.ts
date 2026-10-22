import { DEFAULT_TIMEOUT_MS, timeoutError, timeoutFor, withTimeout } from './call-timeout';

describe('call timeouts', () => {
  it('gives network commands longer than reads', () => {
    expect(timeoutFor('list_watches')).toBe(DEFAULT_TIMEOUT_MS);
    expect(timeoutFor('test_source')).toBeGreaterThan(DEFAULT_TIMEOUT_MS);
    expect(timeoutFor('download_release')).toBeGreaterThan(timeoutFor('test_source'));
  });

  it('names the wait in the error', () => {
    expect(timeoutError(15_000)).toMatchObject({ code: 'unavailable' });
    expect(timeoutError(15_000).message).toContain('15 seconds');
  });

  it('resolves, rejects and times out', async () => {
    await expect(withTimeout(Promise.resolve(1), 50)).resolves.toBe(1);
    await expect(withTimeout(Promise.reject(new Error('x')), 50)).rejects.toThrow('x');
    await expect(withTimeout(new Promise(() => undefined), 10)).rejects.toMatchObject({ code: 'unavailable' });
  });
});
