import { chunkIds } from './chunk-ids';

describe('chunkIds', () => {
  it('keeps order, takes nothing for nothing and one chunk for a few', () => {
    expect(chunkIds([], 1000)).toEqual([]);
    expect(chunkIds(['a', 'b', 'c'], 1000)).toEqual([['a', 'b', 'c']]);
  });

  it('splits by count', () => {
    expect(chunkIds(['a', 'b', 'c', 'd', 'e'], 1000, 2)).toEqual([['a', 'b'], ['c', 'd'], ['e']]);
  });

  it('splits by the bytes of the JSON array so each call stays under the limit', () => {
    const ids = Array.from({ length: 10 }, (_, i) => `model-${i}`.padEnd(40, 'x'));
    const chunks = chunkIds(ids, 200);
    expect(chunks.length).toBeGreaterThan(1);
    for (const c of chunks) expect(new TextEncoder().encode(JSON.stringify(c)).length).toBeLessThanOrEqual(200);
    expect(chunks.flat()).toEqual(ids);
  });

  it('counts bytes, not characters, and puts an id larger than the limit in a call of its own', () => {
    const wide = 'é'.repeat(30); // 60 bytes
    expect(chunkIds([wide, wide, wide], 150).map((c) => c.length)).toEqual([2, 1]);
    expect(chunkIds(['a', 'x'.repeat(500), 'b'], 100)).toEqual([['a'], ['x'.repeat(500)], ['b']]);
  });
});
