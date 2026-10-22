import { readLocal, readLocalJson, writeLocal } from './storage';

const isNumbers = (v: unknown): v is number[] => Array.isArray(v) && v.every((x) => typeof x === 'number');

describe('storage', () => {
  beforeEach(() => localStorage.clear());
  afterEach(() => vi.restoreAllMocks());

  it('round-trips a value and returns null when missing', () => {
    expect(readLocal('k')).toBeNull();
    writeLocal('k', 'v');
    expect(readLocal('k')).toBe('v');
  });

  it('swallows storage errors', () => {
    vi.spyOn(Storage.prototype, 'getItem').mockImplementation(() => {
      throw new Error('blocked');
    });
    vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => {
      throw new Error('blocked');
    });
    expect(readLocal('k')).toBeNull();
    expect(() => writeLocal('k', 'v')).not.toThrow();
  });

  it('reads guarded JSON and falls back on missing, damaged or wrong-shaped data', () => {
    expect(readLocalJson('j', [9], isNumbers)).toEqual([9]);
    writeLocal('j', '[1,2]');
    expect(readLocalJson('j', [9], isNumbers)).toEqual([1, 2]);
    writeLocal('j', '{bad');
    expect(readLocalJson('j', [9], isNumbers)).toEqual([9]);
    writeLocal('j', '["x"]');
    expect(readLocalJson('j', [9], isNumbers)).toEqual([9]);
  });
});
