import { middleEllipsis } from './storage-labels';

describe('middleEllipsis', () => {
  it('keeps short paths and shortens long ones in the middle', () => {
    expect(middleEllipsis('/var/lib/act')).toBe('/var/lib/act');
    const long = '/Users/someone/.cache/act/very/long/nested/folder/name/that/goes/on/and/on/forever';
    const out = middleEllipsis(long, 30);
    expect(out.length).toBe(30);
    expect(out).toContain('…');
    expect(out.startsWith('/Users')).toBe(true);
    expect(out.endsWith('forever')).toBe(true);
  });

  it('does not shorten when the limit is too small to be useful', () => {
    expect(middleEllipsis('/a/b/c/d/e/f/g', 4)).toBe('/a/b/c/d/e/f/g');
  });
});
