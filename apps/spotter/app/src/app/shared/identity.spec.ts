import { hueOf, middleEllipsis, monogram, splitVersion } from './identity';

describe('monogram', () => {
  it.each([
    ['Acme Installer', 'AI'],
    ['lumen-cli', 'LC'],
    ['Node.js', 'NJ'],
    ['Solo', 'S'],
    ['  spaced   out  ', 'SO'],
    ['über straße', 'ÜS'],
  ])('writes %s as %s', (name, expected) => expect(monogram(name)).toBe(expected));

  it('falls back to ? for empty or symbol-only names', () => {
    expect(monogram('')).toBe('?');
    expect(monogram('   ')).toBe('?');
    expect(monogram('---')).toBe('?');
  });
});

describe('hueOf', () => {
  it('is stable, case-insensitive and within range', () => {
    expect(hueOf('Alpha')).toBe(hueOf('alpha'));
    for (const n of ['', 'a', 'Alpha', 'Beta', 'x'.repeat(200), 'ünï']) {
      const h = hueOf(n);
      expect(h).toBeGreaterThanOrEqual(0);
      expect(h).toBeLessThan(360);
      expect(h <= 95 || h >= 155).toBe(true);
    }
  });

  it('differs between names', () => {
    const hues = new Set(['a', 'b', 'c', 'd', 'e', 'f'].map(hueOf));
    expect(hues.size).toBeGreaterThan(3);
  });
});

describe('splitVersion', () => {
  it.each([
    ['R3.10.00T32', 'R3.10.00', 'T32'],
    ['2.1.0-rc.1', '2.1.0', '-rc.1'],
    ['1.0.0+build5', '1.0.0', '+build5'],
    ['2.1.0', '2.1.0', ''],
    ['', '', ''],
    ['latest', 'latest', ''],
  ])('splits %s', (v, main, suffix) => expect(splitVersion(v)).toEqual({ main, suffix }));
});

describe('middleEllipsis', () => {
  it('keeps short text and tiny limits untouched', () => {
    expect(middleEllipsis('abc', 10)).toBe('abc');
    expect(middleEllipsis('abcdefghij', 10)).toBe('abcdefghij');
    expect(middleEllipsis('abcdefghij', 4)).toBe('abcdefghij');
  });

  it('shortens in the middle to exactly max characters', () => {
    const r = middleEllipsis('0123456789abcdef', 9);
    expect(r).toBe('0123…cdef');
    expect(r.length).toBe(9);
  });
});
