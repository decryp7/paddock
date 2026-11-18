import { InjectionToken } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { describe, expect, it } from 'vitest';
import { countingBackend, expectBackendCalls, provideCountingBackend } from './counting-backend';

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
