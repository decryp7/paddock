import type { FinderResult, FoundFile } from '../../core/api.types';
import {
  DEFAULT_MIN_MB,
  MIN_SIZES,
  MODES,
  baseName,
  defaultSelection,
  dirName,
  fileRows,
  groupsEmptied,
  mbToBytes,
  middleEllipsis,
  scanSentence,
  summariseFiles,
} from './files-model';

const MB = 1_000_000;
const f = (id: string, path: string, mb: number, modified = 0): FoundFile => ({ id, path, bytes: mb * MB, modified });

function result(over: Partial<FinderResult> = {}): FinderResult {
  return {
    scanId: 's1',
    folder: '/p',
    large: [f('l1', '/p/a/big.mov', 900), f('d1b', '/p/b/site.psd', 100)],
    duplicates: [
      { bytesEach: 100 * MB, wastedBytes: 200 * MB, files: [f('d1a', '/p/old/site.psd', 100, 1), f('d1b', '/p/b/site.psd', 100, 2), f('d1c', '/p/c/site (1).psd', 100, 3)] },
      { bytesEach: 50 * MB, wastedBytes: 50 * MB, files: [f('d2a', '/p/m.onnx', 50, 1), f('d2b', '/p/x/m.onnx', 50, 2)] },
    ],
    scanned: 1200,
    truncated: false,
    wastedBytes: 250 * MB,
    ...over,
  };
}

describe('files model', () => {
  it('offers sizes, modes and respects the engine floor', () => {
    expect(MIN_SIZES.map((s) => s.label)).toEqual(['50 MB', '100 MB', '500 MB', '1 GB']);
    expect(MIN_SIZES.some((s) => s.mb === DEFAULT_MIN_MB)).toBe(true);
    expect(MODES.map((m) => m.mode)).toEqual(['both', 'large', 'duplicates']);
    expect(mbToBytes(100)).toBe(100 * MB);
  });

  it('selects every copy but the oldest, and no large file', () => {
    expect([...defaultSelection(result())].sort()).toEqual(['d1b', 'd1c', 'd2b']);
    expect(defaultSelection(result({ duplicates: [] })).size).toBe(0);
  });

  it('finds groups whose every copy is selected', () => {
    const r = result();
    expect(groupsEmptied(r, defaultSelection(r))).toEqual([]);
    expect(groupsEmptied(r, new Set(['d2a', 'd2b'])).map((g) => g.files[0]!.id)).toEqual(['d2a']);
    expect(groupsEmptied(r, new Set(['d1a', 'd1b', 'd1c', 'd2a', 'd2b']))).toHaveLength(2);
    expect(groupsEmptied(result({ duplicates: [{ bytesEach: 1, wastedBytes: 0, files: [] }] }), new Set())).toEqual([]);
  });

  it('summarises the selection, counting a file once even when it is in both lists', () => {
    const r = result();
    const s = summariseFiles(r, new Set(['l1', 'd1b', 'd1c']));
    expect(s.count).toBe(3);
    expect(s.bytes).toBe(1100 * MB);
    expect(s.copies).toEqual({ count: 2, bytes: 200 * MB });
    expect(s.large).toEqual({ count: 1, bytes: 900 * MB });
    expect(summariseFiles(null, new Set(['x'])).count).toBe(0);
    expect(summariseFiles(r, new Set()).bytes).toBe(0);
  });

  it('builds one list of rows: large files, then each group, with collapsing', () => {
    const rows = fileRows(result(), new Set());
    expect(rows.filter((r) => r.kind === 'group').map((r) => (r.kind === 'group' ? r.title : ''))).toEqual(['Large files', '3 copies of site.psd', '2 copies of m.onnx']);
    expect(rows.filter((r) => r.kind === 'file' && r.keep).map((r) => (r.kind === 'file' ? r.file.id : ''))).toEqual(['d1a', 'd2a']);
    expect(rows.find((r) => r.kind === 'group')).toMatchObject({ detail: '2 files, 1 GB', open: true });
    expect(rows.find((r) => r.kind === 'group' && r.key === 'g:dup0')).toMatchObject({ detail: '100 MB each, 200 MB wasted' });
    const collapsed = fileRows(result(), new Set(['g:large', 'g:dup1']));
    expect(collapsed.filter((r) => r.kind === 'file')).toHaveLength(3);
    expect(collapsed.filter((r) => r.kind === 'group' && !r.open)).toHaveLength(2);
    expect(fileRows(result({ large: [], duplicates: [] }), new Set())).toEqual([]);
    expect(fileRows(result({ large: [], duplicates: [{ bytesEach: 1, wastedBytes: 0, files: [] }] }), new Set())).toHaveLength(1);
  });

  it('splits and shortens paths', () => {
    expect(baseName('/a/b/c.txt')).toBe('c.txt');
    expect(baseName('C:\\x\\y.bin')).toBe('y.bin');
    expect(baseName('/')).toBe('/');
    expect(dirName('/a/b/c.txt')).toBe('/a/b');
    expect(dirName('C:\\x\\y.bin')).toBe('C:\\x');
    expect(dirName('file.txt')).toBe('');
    expect(middleEllipsis('/short', 20)).toBe('/short');
    const cut = middleEllipsis('/Users/sam/Projects/design-assets/brand-video-final.mov', 30);
    expect(cut).toHaveLength(30);
    expect(cut.endsWith('final.mov')).toBe(true);
    expect(cut).toContain('…');
  });

  it('says what was looked at and whether the walk stopped early', () => {
    expect(scanSentence(result())).toBe('Looked at 1,200 files.');
    expect(scanSentence(result({ scanned: 1, truncated: true }))).toBe('Looked at 1 file before the limit; there may be more.');
  });
});
