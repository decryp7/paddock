import { appendBounded, appendThrottled, prependUnique, upsertBy } from './bounded';

describe('appendBounded', () => {
  it('appends, then keeps the last max', () => {
    expect(appendBounded([1, 2], 3, 5)).toEqual([1, 2, 3]);
    expect(appendBounded([1, 2, 3], 4, 3)).toEqual([2, 3, 4]);
    expect(appendBounded([], 1, 0)).toEqual([1]);
  });

  it('does not change the list it was given', () => {
    const list = [1, 2, 3];
    appendBounded(list, 4, 3);
    expect(list).toEqual([1, 2, 3]);
  });

  it('never grows past its bound however many are appended', () => {
    let list: number[] = [];
    for (let i = 0; i < 1000; i++) list = appendBounded(list, i, 60);
    expect(list).toHaveLength(60);
    expect(list[0]).toBe(940);
    expect(list[59]).toBe(999);
  });
});

describe('appendThrottled', () => {
  const options = { at: (p: { t: number }) => p.t, stepMs: 1000, max: 3 };

  it('takes the first item and then at most one per step', () => {
    let tail: readonly { t: number }[] = [];
    tail = appendThrottled(tail, { t: 0 }, options);
    const same = appendThrottled(tail, { t: 999 }, options);
    expect(same).toBe(tail);
    tail = appendThrottled(tail, { t: 1000 }, options);
    expect(tail.map((p) => p.t)).toEqual([0, 1000]);
  });

  it('is bounded', () => {
    let tail: readonly { t: number }[] = [];
    for (let t = 0; t < 10_000; t += 1000) tail = appendThrottled(tail, { t }, options);
    expect(tail.map((p) => p.t)).toEqual([7000, 8000, 9000]);
  });
});

describe('upsertBy', () => {
  const key = (x: { id: number }) => x.id;

  it('replaces in place by key and puts a new item first', () => {
    const list = [{ id: 1, v: 'a' }, { id: 2, v: 'b' }];
    expect(upsertBy(list, { id: 2, v: 'B' }, key)).toEqual([{ id: 1, v: 'a' }, { id: 2, v: 'B' }]);
    expect(upsertBy(list, { id: 3, v: 'c' }, key)).toEqual([{ id: 3, v: 'c' }, { id: 1, v: 'a' }, { id: 2, v: 'b' }]);
    expect(list).toHaveLength(2);
  });

  it('keeps at most max when given', () => {
    expect(upsertBy([{ id: 1 }, { id: 2 }], { id: 3 }, key, 2)).toEqual([{ id: 3 }, { id: 1 }]);
  });
});

describe('prependUnique', () => {
  const key = (x: { id: string }) => x.id;

  it('puts a new item first, bounded, and returns the same list for one it has', () => {
    const list = [{ id: 'a' }, { id: 'b' }];
    expect(prependUnique(list, { id: 'c' }, key, 2)).toEqual([{ id: 'c' }, { id: 'a' }]);
    expect(prependUnique(list, { id: 'a' }, key, 2)).toBe(list);
    expect(prependUnique([], { id: 'x' }, key, 0)).toEqual([{ id: 'x' }]);
  });
});
