import { describe, expect, it } from 'vitest';
import type { ItemResult } from '../generated/ipc';
import { CommandFailure } from './command-error';
import { isItemOk, parseItemResults, splitItemResults } from './item-result';

const err = { code: 'notFound', message: 'Model gone.', retryable: false } as const;

describe('isItemOk', () => {
  it('tells the two arms apart, also for a null value (a unit reply)', () => {
    expect(isItemOk({ ok: null })).toBe(true);
    expect(isItemOk({ ok: 3 })).toBe(true);
    expect(isItemOk({ err })).toBe(false);
  });
});

describe('parseItemResults', () => {
  it('keeps a well-formed reply of the asked length, errors with known fields only', () => {
    const reply = [{ ok: 1 }, { err: { ...err, extra: 1 } }];
    expect(parseItemResults<number>(reply, 2, (v): v is number => typeof v === 'number')).toEqual([{ ok: 1 }, { err }]);
  });
  it('refuses a reply whose length is not one per item (the command dropped or added items)', () => {
    expect(() => parseItemResults([{ ok: 1 }], 2)).toThrow(CommandFailure);
    expect(() => parseItemResults([{ ok: 1 }], 2)).toThrow(/1 results for 2 items/);
  });
  it('refuses anything that is not an item result', () => {
    for (const bad of [null, {}, 3, [{}], [{ ok: 1, err }], [{ err: { code: 'nope', message: 'x', retryable: true } }], [{ ok: 'x' }]]) {
      const n = Array.isArray(bad) ? bad.length : 1;
      expect(() => parseItemResults(bad, n, (v): v is number => typeof v === 'number')).toThrow(CommandFailure);
    }
  });
  it('the thrown error is internal, so the UI shows it as a contained bug', () => {
    try {
      parseItemResults('x', 1);
      expect.unreachable();
    } catch (e) {
      expect((e as CommandFailure).code).toBe('internal');
    }
  });
});

describe('splitItemResults', () => {
  it('pairs each item with its outcome, for one summary instead of a toast per item', () => {
    const results: ItemResult<number>[] = [{ ok: 10 }, { err }, { ok: 30 }];
    const { ok, failed } = splitItemResults(['a', 'b', 'c'], results);
    expect(ok).toEqual([{ item: 'a', value: 10 }, { item: 'c', value: 30 }]);
    expect(failed).toEqual([{ item: 'b', error: err }]);
  });
  it('refuses lists of different lengths', () => {
    expect(() => splitItemResults(['a'], [])).toThrow(/0 results for 1 items/);
  });
});
