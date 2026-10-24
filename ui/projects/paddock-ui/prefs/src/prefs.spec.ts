import { describe, expect, it } from 'vitest';
import { createPrefs, localPrefs, memoryStorage, type PrefsStorage } from './prefs';

const isCount = (v: unknown): v is { n: number } => typeof v === 'object' && v !== null && typeof (v as { n?: unknown }).n === 'number';

/** A storage whose every call throws, as a blocked or full one does. */
const broken: PrefsStorage = {
  getItem: () => {
    throw new Error('blocked');
  },
  setItem: () => {
    throw new Error('quota');
  },
  removeItem: () => {
    throw new Error('blocked');
  },
};

describe('prefs', () => {
  it('reads back what it wrote, and removes it', () => {
    const p = createPrefs(memoryStorage());
    expect(p.read('a')).toBeNull();
    expect(p.write('a', '1')).toBe(true);
    expect(p.read('a')).toBe('1');
    p.remove('a');
    expect(p.read('a')).toBeNull();
  });

  it('keeps working when the storage throws: reads are empty, writes are not persisted', () => {
    const p = createPrefs(broken);
    expect(p.read('a')).toBeNull();
    expect(p.write('a', '1')).toBe(false);
    expect(p.writeJson('a', { n: 1 })).toBe(false);
    expect(() => p.remove('a')).not.toThrow();
    expect(p.readJson('a', { n: 7 }, isCount)).toEqual({ n: 7 });
  });

  it('falls back on missing, damaged or wrongly shaped JSON', () => {
    const p = createPrefs(memoryStorage({ bad: '{nope', wrong: '{"n":"x"}', ok: '{"n":3}' }));
    expect(p.readJson('missing', { n: 0 }, isCount)).toEqual({ n: 0 });
    expect(p.readJson('bad', { n: 0 }, isCount)).toEqual({ n: 0 });
    expect(p.readJson('wrong', { n: 0 }, isCount)).toEqual({ n: 0 });
    expect(p.readJson('ok', { n: 0 }, isCount)).toEqual({ n: 3 });
    expect(p.writeJson('k', { n: 5 })).toBe(true);
    expect(p.readJson('k', { n: 0 }, isCount)).toEqual({ n: 5 });
  });

  it('does not store a value JSON cannot represent', () => {
    const cyclic: Record<string, unknown> = {};
    cyclic['self'] = cyclic;
    expect(createPrefs(memoryStorage()).writeJson('c', cyclic)).toBe(false);
  });

  it('uses the browser storage by default, and survives one that is not there', () => {
    expect(localPrefs.write('pd.prefs.test', 'x')).toBe(true);
    expect(localPrefs.read('pd.prefs.test')).toBe('x');
    localPrefs.remove('pd.prefs.test');
    const real = Object.getOwnPropertyDescriptor(globalThis, 'localStorage');
    Object.defineProperty(globalThis, 'localStorage', {
      configurable: true,
      get: () => {
        throw new Error('access denied');
      },
    });
    try {
      expect(localPrefs.read('a')).toBeNull();
      expect(localPrefs.write('a', '1')).toBe(false);
    } finally {
      if (real) Object.defineProperty(globalThis, 'localStorage', real);
    }
  });
});
