import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { callTimeoutProblems, DEFAULT_CALL_TIMEOUT_MS, limitFor, timeoutError, withCallTimeout, withTimeout } from './call-timeout';

interface Backend {
  quick(): Promise<string>;
  slow(): Promise<string>;
  startJob(): Promise<string>;
  failing(): Promise<string>;
  subscribe(): () => void;
  readonly name: string;
}

function make(): { backend: Backend; settle: Map<string, (v: string) => void>; fail: (e: unknown) => void } {
  const settle = new Map<string, (v: string) => void>();
  let reject: (e: unknown) => void = () => undefined;
  const pending = (name: string) => new Promise<string>((resolve) => settle.set(name, resolve));
  const backend: Backend = {
    quick: () => pending('quick'),
    slow: () => pending('slow'),
    startJob: () => pending('startJob'),
    failing: () => new Promise<string>((_, r) => (reject = r)),
    subscribe: () => () => undefined,
    name: 'inner',
  };
  return { backend, settle, fail: (e) => reject(e) };
}

beforeEach(() => vi.useFakeTimers());
afterEach(() => vi.useRealTimers());

describe('withCallTimeout', () => {
  it('rejects a call that does not answer in time with a typed, retryable timeout; the late answer is ignored', async () => {
    const { backend, settle } = make();
    const wrapped = withCallTimeout(backend, { subject: 'The Pitwall server' });
    const result = wrapped.quick();
    const caught = result.catch((e: unknown) => e);
    await vi.advanceTimersByTimeAsync(DEFAULT_CALL_TIMEOUT_MS);
    expect(await caught).toEqual(timeoutError('The Pitwall server', 'quick', DEFAULT_CALL_TIMEOUT_MS));
    expect(await caught).toMatchObject({ code: 'timeout', retryable: true, message: expect.stringContaining('did not answer quick within 15 seconds') });
    settle.get('quick')?.('late');
  });

  it('passes an answer that arrives in time and clears its timer', async () => {
    const { backend, settle } = make();
    const wrapped = withCallTimeout(backend, { subject: 'S' });
    const result = wrapped.quick();
    settle.get('quick')?.('ok');
    expect(await result).toBe('ok');
    expect(vi.getTimerCount()).toBe(0);
  });

  it('passes a rejection through unchanged and clears its timer', async () => {
    const { backend, fail } = make();
    const wrapped = withCallTimeout(backend, { subject: 'S' });
    const caught = wrapped.failing().catch((e: unknown) => e);
    const error = { code: 'busy', message: 'later', retryable: true };
    fail(error);
    expect(await caught).toBe(error);
    expect(vi.getTimerCount()).toBe(0);
  });

  it('gives a method its own limit, and never times out an untimed one (a job, rule 12)', async () => {
    const { backend } = make();
    const wrapped = withCallTimeout(backend, { subject: 'S', limits: { slow: 60_000 }, untimed: ['startJob'] });
    const slow = wrapped.slow().catch((e: unknown) => e);
    const job = wrapped.startJob();
    let jobSettled = false;
    void job.then(() => (jobSettled = true));
    await vi.advanceTimersByTimeAsync(59_999);
    expect(vi.getTimerCount()).toBe(1);
    await vi.advanceTimersByTimeAsync(1);
    expect(await slow).toMatchObject({ code: 'timeout', message: expect.stringContaining('60 seconds') });
    await vi.advanceTimersByTimeAsync(3_600_000);
    expect(jobSettled).toBe(false);
  });

  it('passes through what is not a promise: values, and the unsubscribe function of a stream', () => {
    const { backend } = make();
    const wrapped = withCallTimeout(backend, { subject: 'S' });
    expect(wrapped.name).toBe('inner');
    expect(typeof wrapped.subscribe()).toBe('function');
    expect(vi.getTimerCount()).toBe(0);
  });

  it('uses the timers it is given', async () => {
    const { backend } = make();
    const handles: number[] = [];
    const timers = {
      setTimeout: (fn: () => void, ms: number) => {
        handles.push(ms);
        queueMicrotask(fn);
        return 1;
      },
      clearTimeout: () => undefined,
    };
    const wrapped = withCallTimeout(backend, { subject: 'S', defaultMs: 5, timers });
    await expect(wrapped.quick()).rejects.toMatchObject({ code: 'timeout' });
    expect(handles).toEqual([5]);
  });
});

describe('withTimeout and limitFor', () => {
  it('times out a bare promise with the error it is given', async () => {
    const caught = withTimeout(new Promise<string>(() => undefined), 1000, () => timeoutError('S', 'x', 1000)).catch((e: unknown) => e);
    await vi.advanceTimersByTimeAsync(1000);
    expect(await caught).toMatchObject({ code: 'timeout' });
  });
  it('resolves limits: untimed, per method, the policy default, the global default', () => {
    const policy = { subject: 'S', defaultMs: 7, limits: { slow: 9 }, untimed: ['startJob' as const] };
    expect(limitFor<Backend>(policy, 'startJob')).toBeNull();
    expect(limitFor<Backend>(policy, 'slow')).toBe(9);
    expect(limitFor<Backend>(policy, 'quick')).toBe(7);
    expect(limitFor<Backend>({ subject: 'S' }, 'quick')).toBe(DEFAULT_CALL_TIMEOUT_MS);
  });
});

describe('callTimeoutProblems', () => {
  const methods = ['quick', 'slow', 'startJob'];
  it('is empty for a policy that names real methods with positive limits', () => {
    expect(callTimeoutProblems<Backend>(methods, { subject: 'S', limits: { slow: 1 }, untimed: ['startJob'], defaultMs: 5 })).toEqual([]);
  });
  it('names a typo, a bad limit and a bad default', () => {
    const problems = callTimeoutProblems<Backend>(methods, {
      subject: 'S',
      limits: { slw: 5, quick: 0 } as unknown as Partial<Record<keyof Backend, number>>,
      untimed: ['startjob' as keyof Backend],
      defaultMs: -1,
    });
    expect(problems).toEqual([
      'untimed names startjob, which is not a method',
      'limits names slw, which is not a method',
      'the limit of quick must be a positive number of milliseconds',
      'defaultMs must be a positive number of milliseconds',
    ]);
  });
});
