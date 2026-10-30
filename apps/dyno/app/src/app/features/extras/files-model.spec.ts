import type { FinderResult, FoundFile } from '../../core/api.types';
import {
  DEFAULT_MIN_MIB,
  MIN_SIZES,
  MODES,
  baseName,
  dirName,
  fileRows,
  mibToBytes,
  middleEllipsis,
  scanSentence,
} from './files-model';

const MB = 1024 * 1024;
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
    unreadable: 0,
    limit: null,
    wastedBytes: 250 * MB,
    ...over,
  };
}

describe('files model', () => {
  it('offers sizes, modes and respects the engine floor', () => {
    expect(MIN_SIZES.map((s) => s.label)).toEqual(['50 MB', '100 MB', '500 MB', '1 GB']);
    expect(MIN_SIZES.some((s) => s.mib === DEFAULT_MIN_MIB)).toBe(true);
    expect(MODES.map((m) => m.mode)).toEqual(['both', 'large', 'duplicates']);
    expect(mibToBytes(100)).toBe(100 * MB);
  });

  it('builds one list of rows: large files, then each group, with collapsing', () => {
    const rows = fileRows(result(), new Set());
    expect(rows.filter((r) => r.kind === 'group').map((r) => (r.kind === 'group' ? r.title : ''))).toEqual(['Large files', '3 copies of site.psd', '2 copies of m.onnx']);
    expect(rows.find((r) => r.kind === 'group')).toMatchObject({ detail: '2 files, 0.98 GB', open: true });
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
    const limit = { limit: 'files' as const, message: 'Stopped after 500000 files and folders. Choose a smaller folder to see everything in it.' };
    expect(scanSentence(result({ scanned: 1, limit }))).toBe(`Looked at 1 file before a limit. ${limit.message}`);
    expect(scanSentence(result({ unreadable: 1 }))).toBe('Looked at 1,200 files. 1 folder could not be read and was left out.');
    expect(scanSentence(result({ unreadable: 3 }))).toBe('Looked at 1,200 files. 3 folders could not be read and were left out.');
  });
});
