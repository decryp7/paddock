import { DEFAULT_TIMEOUT_MS, isJobCommand, timeoutError, timeoutFor, withTimeout } from './call-timeout';

describe('call timeouts', () => {
  it('gives updates and imports longer than reads', () => {
    expect(timeoutFor('list_watches')).toBe(DEFAULT_TIMEOUT_MS);
    expect(timeoutFor('discover_feeds')).toBeGreaterThan(DEFAULT_TIMEOUT_MS);
    expect(timeoutFor('install_app_update')).toBeGreaterThan(timeoutFor('discover_feeds') as number);
  });

  it('never times out a long operation: the engine ends it and the UI shows what it is doing', () => {
    for (const cmd of ['download_release', 'install_renderer', 'check_now', 'test_source']) {
      expect(timeoutFor(cmd), cmd).toBeNull();
      expect(isJobCommand(cmd), cmd).toBe(true);
    }
    expect(isJobCommand('list_watches')).toBe(false);
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
