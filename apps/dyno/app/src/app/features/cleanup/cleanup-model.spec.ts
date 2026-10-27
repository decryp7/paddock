import type { CategoryResult, CleanupCategory, CleanupItem, CleanupScan } from '../../core/api.types';
import { baseName, defaultPicks, groupCategories, initialSelection, isStale, outcomeSentence, selectionState, summarise, SCAN_VALID_MS } from './cleanup-model';

const cat = (id: string, group: CleanupCategory['group'], risk: CleanupCategory['risk'] = 'safe'): CleanupCategory => ({ id, name: id, description: '', group, risk, scheduled: false });
const item = (id: string, bytes: number, selected = true): CleanupItem => ({ id, path: `/t/${id}`, bytes, modified: 0, folder: false, reason: '', selected });
const result = (category: string, items: CleanupItem[]): CategoryResult => ({ category, bytes: items.reduce((a, i) => a + i.bytes, 0), itemCount: items.length, items, truncated: false, note: null });
const scan = (at = 1000): CleanupScan => ({ id: 's', at, results: [result('a', [item('a1', 10), item('a2', 20, false)]), result('b', [item('b1', 5)])], totalBytes: 35 });

describe('cleanup model', () => {
  it('groups categories in the page order and drops empty groups', () => {
    const groups = groupCategories([cat('x', 'developer'), cat('y', 'system'), cat('z', 'developer')]);
    expect(groups.map((g) => g.group)).toEqual(['system', 'developer']);
    expect(groups[1]!.categories.map((c) => c.id)).toEqual(['x', 'z']);
  });

  it('scans only the safe categories by default', () => {
    expect(defaultPicks([cat('a', 'system'), cat('b', 'developer', 'review'), cat('c', 'downloads', 'personal')])).toEqual(['a']);
  });

  it('starts from what the engine pre-selected', () => {
    expect(initialSelection(scan())).toEqual(['a1', 'b1']);
  });

  it('summarises the selection in total and per category, leaving out empty ones', () => {
    const s = summarise(scan(), new Set(['a2', 'b1']));
    expect(s).toEqual({ count: 2, bytes: 25, byCategory: [{ category: 'a', count: 1, bytes: 20 }, { category: 'b', count: 1, bytes: 5 }] });
    expect(summarise(scan(), new Set()).byCategory).toEqual([]);
    expect(summarise(null, new Set(['a1'])).count).toBe(0);
  });

  it('knows when a scan is too old for the engine', () => {
    expect(isStale(scan(1000), 1000 + SCAN_VALID_MS)).toBe(false);
    expect(isStale(scan(1000), 1001 + SCAN_VALID_MS)).toBe(true);
    expect(isStale(null, 1e12)).toBe(false);
  });

  it('reports none, some or all selected', () => {
    const r = scan().results[0]!;
    expect(selectionState(r, new Set())).toBe('none');
    expect(selectionState(r, new Set(['a1']))).toBe('some');
    expect(selectionState(r, new Set(['a1', 'a2']))).toBe('all');
    expect(selectionState(result('e', []), new Set())).toBe('none');
  });

  it('says plainly what a run did', () => {
    const out = { freedBytes: 2048, deleted: 2, failed: [], quarantineId: 'q' };
    expect(outcomeSentence(out, 'quarantine')).toBe('Moved 2 items (2 KB) to the 7-day quarantine.');
    expect(outcomeSentence({ ...out, deleted: 1 }, 'recycleBin')).toContain('1 item (2 KB) to the Recycle Bin / Trash');
    expect(outcomeSentence({ ...out, deleted: 0 }, 'quarantine')).toBe('Nothing was moved.');
  });

  it('takes the last path segment for either separator', () => {
    expect(baseName('C:\\a\\b.tmp')).toBe('b.tmp');
    expect(baseName('/a/b/')).toBe('b');
    expect(baseName('/')).toBe('/');
  });
});
