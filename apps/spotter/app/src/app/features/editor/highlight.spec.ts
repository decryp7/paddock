import { groupClass, groupLabel, segments } from './highlight';

describe('highlight', () => {
  it('cuts a name by where the groups matched', () => {
    const s = segments('Acme-PRO-R3.zip', [
      { group: 'id', start: 5, end: 8 },
      { group: 'major', start: 10, end: 11 },
    ]);
    expect(s.map((x) => [x.text, x.group])).toEqual([
      ['Acme-', null],
      ['PRO', 'id'],
      ['-R', null],
      ['3', 'major'],
      ['.zip', null],
    ]);
  });

  it('lets the innermost group colour nested text and clips spans to the name', () => {
    const s = segments('v12', [
      { group: 'ver', start: 0, end: 99 },
      { group: 'major', start: 1, end: 3 },
    ]);
    expect(s.map((x) => [x.text, x.group])).toEqual([['v', 'ver'], ['12', 'major']]);
    expect(segments('', [])).toEqual([]);
    expect(segments('abc', [])).toEqual([{ text: 'abc', group: null }]);
    expect(segments('ab', [{ group: 'id', start: -3, end: 1 }])[0]).toEqual({ text: 'a', group: 'id' });
  });

  it('names and classes the groups', () => {
    expect(groupLabel('id')).toBe('product id');
    expect(groupLabel('minor')).toBe('minor number');
    expect(groupLabel('pre')).toBe('channel');
    expect(groupLabel('variant')).toBe('variant');
    expect(groupLabel('ext')).toBe('field "ext"');
    expect(['id', 'build', 'channel', 'variant', 'ext'].map(groupClass)).toEqual(['g-id', 'g-num', 'g-channel', 'g-variant', 'g-field']);
  });
});
