import { DEFAULT_TIMEOUT_MS, timeoutError, timeoutFor, withTimeout } from './call-timeout';
import { toCommandError } from './command-error';
import { TauriBackend, type CommandCall, type EventListen } from './tauri-backend';

describe('call timeouts', () => {
  afterEach(() => vi.useRealTimers());

  it('gives long jobs more time than ordinary calls', () => {
    expect(timeoutFor('runner_status')).toBe(DEFAULT_TIMEOUT_MS);
    expect(timeoutFor('runner_disk')).toBeGreaterThan(DEFAULT_TIMEOUT_MS);
    expect(timeoutFor('clean_disk')).toBeGreaterThan(timeoutFor('runner_disk'));
    expect(timeoutFor('register_runner')).toBe(timeoutFor('clean_disk'));
  });

  it('names the wait in the message and survives the error mapping', () => {
    const e = timeoutError(15_000);
    expect(e.code).toBe('timeout');
    expect(e.message).toContain('15 seconds');
    expect(toCommandError(e)).toEqual(e);
  });

  it('passes answers and errors through and clears its timer', async () => {
    vi.useFakeTimers();
    await expect(withTimeout(Promise.resolve(1), 1000)).resolves.toBe(1);
    await expect(withTimeout(Promise.reject(new Error('x')), 1000)).rejects.toThrow('x');
    expect(vi.getTimerCount()).toBe(0);
  });

  it('stops waiting for a call that never answers, and ignores its late answer', async () => {
    vi.useFakeTimers();
    let answer!: (v: string) => void;
    const slow = new Promise<string>((r) => (answer = r));
    const outcome = withTimeout(slow, 1000).catch((e: unknown) => e);
    await vi.advanceTimersByTimeAsync(1001);
    expect(await outcome).toMatchObject({ code: 'timeout' });
    answer('late');
  });

  it('the Tauri backend rejects a hung command with a timeout CommandError after its budget', async () => {
    vi.useFakeTimers();
    const call = vi.fn(() => new Promise(() => undefined));
    const backend = new TauriBackend(call as unknown as CommandCall, vi.fn() as unknown as EventListen);
    const status = backend.status().catch((e: unknown) => e);
    await vi.advanceTimersByTimeAsync(DEFAULT_TIMEOUT_MS + 1);
    expect(await status).toMatchObject({ code: 'timeout' });
    const clean = backend.cleanDisk('r').catch((e: unknown) => e);
    await vi.advanceTimersByTimeAsync(DEFAULT_TIMEOUT_MS + 1);
    let settled = false;
    void clean.then(() => (settled = true));
    await vi.advanceTimersByTimeAsync(0);
    expect(settled).toBe(false);
    await vi.advanceTimersByTimeAsync(timeoutFor('clean_disk'));
    expect(await clean).toMatchObject({ code: 'timeout' });
  });
});
