import { InjectionToken, computed, signal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { describe, expect, it } from 'vitest';
import { countingBackend, countingInvoke, expectBackendCalls, expectInvokes, provideCountingBackend } from './counting-backend';

interface Backend {
  deleteModel(id: string): Promise<void>;
  deleteModels(ids: readonly string[]): Promise<void>;
  jobs: { cancel(id: number): Promise<void> };
  readonly name: string;
}

class Fake implements Backend {
  readonly #deleted: string[] = [];
  readonly name = 'fake';
  readonly jobs = { cancel: async (_id: number) => undefined };
  async deleteModel(id: string): Promise<void> {
    this.#deleted.push(id);
  }
  async deleteModels(ids: readonly string[]): Promise<void> {
    this.#deleted.push(...ids);
  }
  deleted(): readonly string[] {
    return this.#deleted;
  }
}

const BACKEND = new InjectionToken<Backend>('Backend');

/** The code under test: the per-item loop the bulk-API redesign removes, and its bulk form. */
async function cleanOneByOne(b: Backend, ids: readonly string[]): Promise<void> {
  for (const id of ids) await b.deleteModel(id);
}
async function cleanAtOnce(b: Backend, ids: readonly string[]): Promise<void> {
  await b.deleteModels(ids);
}

describe('countingBackend', () => {
  it('counts every call by method, nested ports too, and passes calls through to the inner backend (private fields work)', async () => {
    const inner = new Fake();
    const counter = countingBackend<Backend>(inner);
    await counter.backend.deleteModel('a');
    await counter.backend.jobs.cancel(1);
    expect(counter.backend.name).toBe('fake');
    expect(counter.count()).toBe(2);
    expect(counter.count('deleteModel')).toBe(1);
    expect(counter.count('jobs.cancel')).toBe(1);
    expect(counter.calls).toEqual([
      { method: 'deleteModel', args: ['a'] },
      { method: 'jobs.cancel', args: [1] },
    ]);
    expect(inner.deleted()).toEqual(['a']);
    counter.reset();
    expect(counter.count()).toBe(0);
  });

  it('does not count signal reads or wrap subscribables, and keeps the inner object as the receiver of getters', async () => {
    const items = signal([1, 2]);
    const inner = {
      items,
      total: computed(() => items().length),
      changes: { subscribe: (_f: unknown) => ({ unsubscribe: () => undefined }), pipe: () => inner.changes },
      get onInner(): boolean {
        return this === inner;
      },
      async load() {
        return items();
      },
    };
    const counter = countingBackend(inner);
    expect(counter.backend.items()).toEqual([1, 2]);
    expect(counter.backend.total()).toBe(2);
    counter.backend.changes.subscribe(() => undefined);
    expect(counter.backend.changes).toBe(inner.changes);
    expect(counter.backend.onInner).toBe(true);
    await counter.backend.load();
    expect(counter.calls.map((c) => c.method)).toEqual(['load']);
  });

  it('is scoped to its own instance: two counters never see each other', async () => {
    const a = countingBackend<Backend>(new Fake());
    const b = countingBackend<Backend>(new Fake());
    await a.backend.deleteModel('x');
    expect(b.count()).toBe(0);
  });
});

describe('expectBackendCalls (assertNumQueries for the backend)', () => {
  it('passes when the code makes exactly the expected calls, and returns its value', async () => {
    const counter = countingBackend<Backend>(new Fake());
    await expect(expectBackendCalls(counter, 1, () => cleanAtOnce(counter.backend, ['a', 'b', 'c']).then(() => 'done'))).resolves.toBe('done');
  });

  it('positive control (ER-33): the per-item loop fails the same assertion, naming the calls', async () => {
    const counter = countingBackend<Backend>(new Fake());
    await expect(expectBackendCalls(counter, 1, () => cleanOneByOne(counter.backend, ['a', 'b', 'c']))).rejects.toThrow(
      /expected 1 backend call, got 3: deleteModel, deleteModel, deleteModel/,
    );
  });

  it('takes a budget ({ max }) and a method filter', async () => {
    const counter = countingBackend<Backend>(new Fake());
    await expectBackendCalls(counter, { max: 3 }, () => cleanOneByOne(counter.backend, ['a', 'b', 'c']));
    await expect(expectBackendCalls(counter, { max: 2 }, () => cleanOneByOne(counter.backend, ['a', 'b', 'c']))).rejects.toThrow(/at most 2 backend calls, got 3/);
    await expectBackendCalls(counter, 0, () => counter.backend.jobs.cancel(1), 'deleteModel');
  });

  it('a budget ({ max }) fails on zero calls (positive control: the work went to another backend instance) unless allowNone says why', async () => {
    const counter = countingBackend<Backend>(new Fake());
    const other = new Fake();
    await expect(expectBackendCalls(counter, { max: 3 }, () => cleanOneByOne(other, ['a', 'b']))).rejects.toThrow(/at least one backend call, got 0/);
    await expectBackendCalls(counter, { max: 3, allowNone: 'nothing is unused, so nothing is deleted' }, () => cleanOneByOne(counter.backend, []));
    await expect(expectBackendCalls(counter, { max: 3, allowNone: ' ' }, () => Promise.resolve())).rejects.toThrow(/allowNone needs a reason/);
  });

  it('counts only the calls the code made, not the ones before', async () => {
    const counter = countingBackend<Backend>(new Fake());
    await counter.backend.deleteModel('before');
    await expectBackendCalls(counter, 1, () => cleanAtOnce(counter.backend, ['a']));
  });

  it('works with a synchronous body', async () => {
    const counter = countingBackend<Backend>(new Fake());
    expect(await expectBackendCalls(counter, 1, () => (void counter.backend.deleteModel('a'), 7))).toBe(7);
  });
});

describe('provideCountingBackend', () => {
  it('provides the counted backend through the app\'s own DI token, so stores and components are counted unchanged', async () => {
    const { providers, counter } = provideCountingBackend(BACKEND, new Fake());
    TestBed.configureTestingModule({ providers });
    const injected = TestBed.inject(BACKEND);
    await expectBackendCalls(counter, 3, () => cleanOneByOne(injected, ['a', 'b', 'c']));
    expect(injected).toBe(counter.backend);
  });
});

/** The shape of an app's `tauri-backend.ts`: a bridge whose `invoke` is the IPC call, passed in (the real one by default). */
interface Bridge {
  invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T>;
}
class TauriLikeBackend {
  constructor(private readonly bridge: Bridge) {}
  /** The per-item shape the lint's `parallel` finding names: one invoke per id, joined. */
  async getEach(ids: readonly number[]): Promise<unknown[]> {
    return Promise.all(ids.map((id) => this.bridge.invoke('get_item', { id })));
  }
  async getAll(ids: readonly number[]): Promise<unknown[]> {
    return this.bridge.invoke('get_items', { ids });
  }
}

describe('countingInvoke (the IPC call itself, under a tauri-backend)', () => {
  const fakeInvoke = async <T>(cmd: string, args?: Record<string, unknown>): Promise<T> => ({ cmd, args }) as T;

  it('counts every invoke by command and passes it through', async () => {
    const counter = countingInvoke(fakeInvoke);
    const backend = new TauriLikeBackend({ invoke: counter.invoke });
    expect(await backend.getAll([1, 2])).toEqual({ cmd: 'get_items', args: { ids: [1, 2] } });
    expect(counter.count()).toBe(1);
    expect(counter.count('get_items')).toBe(1);
    expect(counter.calls).toEqual([{ command: 'get_items', args: { ids: [1, 2] } }]);
  });

  it('positive control (ER-33): Promise.all over ids.map(invoke) fails a budget of 1; the bulk command meets it', async () => {
    const counter = countingInvoke(fakeInvoke);
    const backend = new TauriLikeBackend({ invoke: counter.invoke });
    await expect(expectInvokes(counter, 1, () => backend.getEach([1, 2, 3]))).rejects.toThrow(/expected 1 invoke, got 3: get_item, get_item, get_item/);
    await expectInvokes(counter, 1, () => backend.getAll([1, 2, 3]));
    await expect(expectInvokes(counter, { max: 2 }, () => Promise.resolve())).rejects.toThrow(/at least one invoke, got 0/);
    await expectInvokes(counter, 0, () => backend.getAll([1]), 'get_item');
  });
});
