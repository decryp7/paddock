import { MockBackend } from './mock-backend';
import { DEFAULT_TIMEOUT_MS, timeoutError, withTimeout } from './timeout-backend';

describe('withTimeout', () => {
  afterEach(() => vi.useRealTimers());

  it('passes results and errors through', async () => {
    const backend = withTimeout(new MockBackend({ failing: ['events'] }));
    expect((await backend.localModels()).length).toBeGreaterThan(0);
    await expect(backend.events(5)).rejects.toMatchObject({ code: 'serverUnavailable' });
  });

  it('turns a call that never answers into a typed error with a hint, after the timeout', async () => {
    vi.useFakeTimers();
    const hung = { overview: () => new Promise(() => undefined) } as unknown as MockBackend;
    const pending = withTimeout(hung, 1000).overview();
    const assertion = expect(pending).rejects.toEqual(timeoutError('overview', 1000));
    await vi.advanceTimersByTimeAsync(1000);
    await assertion;
    expect(timeoutError('x', DEFAULT_TIMEOUT_MS).message).toContain('15 seconds');
  });

  it('gives slow calls longer, and leaves streaming calls and plain values alone', async () => {
    vi.useFakeTimers();
    const slow = { loadModel: () => new Promise<void>((r) => setTimeout(r, 5000)), name: 'x', subscribe: () => () => undefined } as unknown as MockBackend;
    const wrapped = withTimeout(slow, 1000);
    const done = wrapped.loadModel('m');
    await vi.advanceTimersByTimeAsync(5000);
    await expect(done).resolves.toBeUndefined();
    expect(typeof wrapped.subscribe(() => undefined)).toBe('function');
    expect((wrapped as unknown as { name: string }).name).toBe('x');
  });

  it('never times out a job, however long it runs (ER-19)', async () => {
    vi.useFakeTimers();
    let finish: () => void = () => undefined;
    const inner = { loadModel: () => new Promise<void>((r) => (finish = r)) } as unknown as MockBackend;
    let settled = false;
    const pending = withTimeout(inner, 1000).loadModel('m').finally(() => (settled = true));
    await vi.advanceTimersByTimeAsync(3_600_000);
    expect(settled).toBe(false);
    finish();
    await pending;
    expect(settled).toBe(true);
  });
});
