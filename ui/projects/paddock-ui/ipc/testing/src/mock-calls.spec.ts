import { afterEach, describe, expect, it, vi } from 'vitest';
import { withMockOptions } from './mock-calls';
import type { MockOptions } from './mock-options';

interface Api {
  load(id: number): Promise<string>;
  save(): Promise<string>;
  watch(): () => void;
}

const inner = (): Api => ({ load: async (id) => `loaded ${id}`, save: async () => 'saved', watch: () => () => undefined });
const options = (over: Partial<MockOptions> = {}): MockOptions => ({ scenarios: [], latencyMs: 0, fail: {}, hold: [], ...over });

afterEach(() => {
  delete (globalThis as { __paddockMockRelease?: unknown }).__paddockMockRelease;
});

describe('withMockOptions', () => {
  it('answers like the inner backend and records the calls, including non-promise ones', async () => {
    const { backend, calls } = withMockOptions(inner(), options());
    expect(await backend.load(3)).toBe('loaded 3');
    expect(typeof backend.watch()).toBe('function');
    expect(calls).toEqual([{ method: 'load', args: [3] }, { method: 'watch', args: [] }]);
  });

  it('waits the latency before answering', async () => {
    const sleeps: number[] = [];
    const { backend } = withMockOptions(inner(), options({ latencyMs: 150 }), { sleep: async (ms) => void sleeps.push(ms) });
    await backend.save();
    expect(sleeps).toEqual([150]);
  });

  it('fails the first N calls of a command, or every call, with the typed error', async () => {
    const { backend } = withMockOptions(inner(), options({ fail: { load: 2, save: Infinity } }));
    await expect(backend.load(1)).rejects.toMatchObject({ code: 'internal', retryable: true });
    await expect(backend.load(1)).rejects.toMatchObject({ code: 'internal' });
    expect(await backend.load(1)).toBe('loaded 1');
    for (let i = 0; i < 3; i++) await expect(backend.save()).rejects.toMatchObject({ message: expect.stringContaining('save failed on purpose') });
  });

  it('uses the failure the app asks for, and ignores a rejection of the inner call that failed anyway', async () => {
    const api: Api = { ...inner(), save: () => Promise.reject(new Error('inner')) };
    const { backend } = withMockOptions(api, options({ fail: { save: 1 } }), { failure: (m) => ({ code: 'busy', message: m, retryable: true }) });
    await expect(backend.save()).rejects.toMatchObject({ code: 'busy', message: 'save' });
    await expect(backend.save()).rejects.toThrow('inner');
  });

  it('holds the answers of a command until it is released, and passes later calls straight through', async () => {
    const { backend, release } = withMockOptions(inner(), options({ hold: ['load'] }));
    let answered = '';
    const first = backend.load(1).then((v) => (answered = v));
    const second = backend.load(2);
    await Promise.resolve();
    expect(answered).toBe('');
    expect(await backend.save()).toBe('saved');
    release('load');
    await Promise.all([first, second]);
    expect(answered).toBe('loaded 1');
    expect(await backend.load(3)).toBe('loaded 3');
  });

  it('releases every held command from the page-level hook', async () => {
    const { backend } = withMockOptions(inner(), options({ hold: ['load', 'save'] }));
    const done = Promise.all([backend.load(1), backend.save()]);
    await Promise.resolve();
    (globalThis as unknown as { __paddockMockRelease: (m?: string) => void }).__paddockMockRelease();
    expect(await done).toEqual(['loaded 1', 'saved']);
  });

  it('really sleeps with the default port', async () => {
    vi.useFakeTimers();
    try {
      const { backend } = withMockOptions(inner(), options({ latencyMs: 20 }));
      const answer = backend.save();
      await vi.advanceTimersByTimeAsync(20);
      expect(await answer).toBe('saved');
    } finally {
      vi.useRealTimers();
    }
  });
});
