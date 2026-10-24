import { queryMarks, didYouMean, editDistance, marksOf, matchWord, plain, rank, showsCategory, suggestionScore, titleParts, tolerance, type Rankable } from './match';

const item = (id: string, title: string, category?: string): Rankable => ({ id, title, ...(category ? { category } : {}) });

describe('palette matching (ported from Debrief F1)', () => {
  it('matches word starts, longer substrings and initials, not scattered letters', () => {
    expect(matchWord('anom', 'Show anomalies')?.indices).toEqual([5, 6, 7, 8]);
    expect(matchWord('anom', 'Full screen does: hide tool windows')).toBeNull();
    expect(matchWord('anom', 'Previous time range')).toBeNull();
    expect(matchWord('mal', 'Show anomalies')?.indices).toEqual([8, 9, 10]);
    expect(matchWord('ma', 'Show anomalies')).toBeNull();
    expect(matchWord('sa', 'Show anomalies')?.indices).toEqual([0, 5]);
    expect(matchWord('s', 'Show anomalies')?.indices).toEqual([0]);
    expect(matchWord('', 'x')).toEqual({ score: 0, indices: [] });
    expect(matchWord('time', 'Previous time range')!.score).toBeGreaterThan(matchWord('time', 'Lifetime')!.score);
    expect(matchWord('ai', 'Open an image')).toBeNull();
    expect(matchWord('ai', 'AI: Manage models')?.indices).toEqual([0, 1]);
    expect(matchWord('xyz', 'ab')).toBeNull();
  });

  it('shows the category once', () => {
    expect(showsCategory({ id: 'a', title: 'AI: Open the assistant chat', category: 'AI' })).toBe(false);
    expect(showsCategory({ id: 'a', title: 'View: Jobs', category: 'Window' })).toBe(true);
    expect(showsCategory({ id: 'a', title: 'Open folder' })).toBe(false);
    expect(plain(item('a', 'Open folder', 'File'))).toMatchObject({ showCategory: true });
  });

  it('ranks by every query word, better matches first, and rejects loose matches', () => {
    const items = [
      item('a', 'Full screen does: hide tool windows', 'View'),
      item('b', 'Previous time range', 'Analyze'),
      item('c', 'Show anomalies', 'Analyze'),
      item('d', 'Open Anomalies', 'View'),
      item('e', 'Theme: console', 'View'),
    ];
    expect(rank(items, 'anom').map((e) => e.item.id)).toEqual(['c', 'd']);
    expect(rank(items, 'open anom').map((e) => e.item.id)).toEqual(['d']);
    expect(rank(items, 'view: theme').map((e) => e.item.id)).toEqual(['e']);
    expect(rank(items, 'analyze anom').map((e) => e.item.id)).toEqual(['c']);
    expect(rank(items, 'zzz')).toEqual([]);
    expect(titleParts('Open Anomalies', rank(items, 'anom')[1]!.marks)).toEqual([
      { text: 'Open ', mark: false },
      { text: 'Anom', mark: true },
      { text: 'alies', mark: false },
    ]);
  });

  it('ranks familiar items first among equals, and lists recent then common for an empty query', () => {
    const items = [item('m', 'AI: Manage models', 'AI'), item('i', 'Open an image'), item('s', 'Set up AI assistant', 'AI'), item('c', 'AI: Open the assistant chat', 'AI')];
    expect(rank(items, 'ai').map((e) => e.item.id)).toEqual(['m', 'c', 's']);
    expect(rank(items, 'ai', [], ['s'])[0]!.item.id).toBe('s');
    expect(rank(items, 'ai', ['c'], [])[0]!.item.id).toBe('c');
    const list = [item('x', 'Capture'), item('y', 'Open folder…'), item('z', 'Search'), item('w', 'Report')];
    expect(rank(list, '  ', ['z'], ['y', 'missing']).map((e) => e.item.id)).toEqual(['z', 'y', 'x', 'w']);
    expect(rank(list, '').map((e) => e.item.id)).toEqual(['x', 'y', 'z', 'w']);
  });

  it('turns provider highlight ranges into marks, clamped', () => {
    expect([...marksOf('main.rs', [[0, 2], [5, 99], [-3, 0]])]).toEqual([0, 1, 5, 6]);
    expect(marksOf('x', undefined).size).toBe(0);
  });
});

describe('did you mean', () => {
  it('measures edit distance case-insensitively, counting adjacent swaps once', () => {
    expect(editDistance('Export', 'export')).toBe(0);
    expect(editDistance('', 'abc')).toBe(3);
    expect(editDistance('abc', '')).toBe(3);
    expect(editDistance('exprot', 'export')).toBe(1);
    expect(editDistance('kitten', 'sitting')).toBe(3);
    expect(editDistance('ab', 'ba')).toBe(1);
  });

  it('tolerates one typo per three characters, at least one', () => {
    expect(tolerance('ab')).toBe(1);
    expect(tolerance('timeline')).toBe(2);
  });

  it('scores every query word against its closest label word', () => {
    expect(suggestionScore('exprot', 'File Export events')).toBe(1);
    expect(suggestionScore('exp', 'Export events')).toBe(0);
    expect(suggestionScore('zzzz', 'Export events')).toBeNull();
    expect(suggestionScore('', 'Export')).toBeNull();
    expect(suggestionScore('x', '…')).toBeNull();
  });

  it('ranks items best first, keeps ties in order and limits the list', () => {
    const items = ['Toggle timeline panel', 'Toggle sidebar', 'Show timeline', 'Open folder'];
    expect(didYouMean(items, 'timelnie', (x) => x)).toEqual(['Toggle timeline panel', 'Show timeline']);
    expect(didYouMean(items, 'toggel', (x) => x, 1)).toEqual(['Toggle timeline panel']);
    expect(didYouMean(items, 'qqqqqq', (x) => x)).toEqual([]);
  });
});

describe('queryMarks', () => {
  it('marks where the words of a query land in a label', () => {
    expect([...queryMarks('handle req', 'handle_request')]).toEqual([0, 1, 2, 3, 4, 5, 7, 8, 9]);
    expect(queryMarks('', 'x').size).toBe(0);
    expect(queryMarks('zz', 'abc').size).toBe(0);
  });
});
