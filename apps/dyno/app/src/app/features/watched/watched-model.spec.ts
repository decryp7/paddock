import type { SweepPlan, SweepSummary, WatchedFolderView } from '../../core/api.types';
import { KEEP_PRESETS, dueRow, dueSummary, folderName, keepLabel, keepOptions, shortPath, staysText, statusOf, sweepText } from './watched-model';

const NOW = 1_700_000_000_000;
const MIN = 60_000;

function view(over: Partial<WatchedFolderView['folder']> = {}, exists = true): WatchedFolderView {
  return {
    folder: {
      id: 'w-1',
      path: '/Users/me/Downloads',
      keepForSecs: 604_800,
      filter: { extensions: [], nameStarts: [] },
      recursive: false,
      enabled: false,
      previewed: false,
      template: null,
      added: NOW,
      lastSweep: null,
      ...over,
    },
    exists,
    filterText: 'every file',
    nextSweep: null,
  };
}

const sweep = (over: Partial<SweepSummary> = {}): SweepSummary => ({ at: NOW - 5 * MIN, moved: 0, bytes: 0, failed: 0, firstFailure: null, quarantineId: null, ...over });
const plan = (over: Partial<SweepPlan> = {}): SweepPlan => ({ due: [], dueTotal: 0, dueBytes: 0, waiting: 0, other: 0, links: 0, refused: 0, truncated: false, ...over });

describe('keep times', () => {
  it('are worded like the engine words them', () => {
    expect(keepLabel(600)).toBe('10 minutes');
    expect(keepLabel(60)).toBe('1 minute');
    expect(keepLabel(3600)).toBe('1 hour');
    expect(keepLabel(7200)).toBe('2 hours');
    expect(keepLabel(86_400)).toBe('1 day');
    expect(keepLabel(864_000)).toBe('10 days');
    expect(keepLabel(90 * 60)).toBe('90 minutes');
  });
  it('the presets cover the engine range and a custom value stays selectable', () => {
    expect(KEEP_PRESETS[0]!.secs).toBe(600);
    expect(KEEP_PRESETS.at(-1)!.secs).toBe(864_000);
    expect(keepOptions(86_400)).toHaveLength(KEEP_PRESETS.length);
    const o = keepOptions(5 * 86_400);
    expect(o.map((p) => p.secs)).toEqual([...o.map((p) => p.secs)].sort((a, b) => a - b));
    expect(o.find((p) => p.secs === 5 * 86_400)?.label).toBe('5 days');
  });
});

describe('paths', () => {
  it('names a folder by its last segment with either separator', () => {
    expect(folderName('/Users/me/Downloads')).toBe('Downloads');
    expect(folderName('C:\\Users\\me\\Downloads\\')).toBe('Downloads');
    expect(folderName('/')).toBe('/');
  });
  it('shortens in the middle and keeps both ends', () => {
    expect(shortPath('/a/b')).toBe('/a/b');
    const s = shortPath('/Users/alexander/Projects/very/deep/folder/structure/build-output', 30);
    expect(s).toHaveLength(30);
    expect(s.startsWith('/Users/alex')).toBe(true);
    expect(s.endsWith('build-output')).toBe(true);
    expect(s).toContain('…');
  });
});

describe('status', () => {
  it('is calm when on or off and warns on exceptions', () => {
    expect(statusOf(view({ enabled: true }))).toEqual({ tone: 'ok', text: 'On' });
    expect(statusOf(view({ previewed: true }))).toEqual({ tone: null, text: 'Off' });
    expect(statusOf(view())).toEqual({ tone: null, text: 'Off, not previewed' });
    expect(statusOf(view({ enabled: true }, false))).toEqual({ tone: 'warn', text: 'Folder missing' });
    expect(statusOf(view({ enabled: true, lastSweep: sweep({ failed: 1, firstFailure: 'x' }) }))).toEqual({ tone: 'warn', text: 'Could not sweep' });
    // Some files stayed but others moved: still on.
    expect(statusOf(view({ enabled: true, lastSweep: sweep({ moved: 2, failed: 1, firstFailure: 'x' }) })).tone).toBe('ok');
  });
});

describe('sweepText', () => {
  it('says what the last sweep did', () => {
    expect(sweepText(null, NOW)).toBe('Not yet');
    expect(sweepText(sweep(), NOW)).toBe('5 min ago: nothing due');
    expect(sweepText(sweep({ moved: 3, bytes: 5 * 1000 * 1000 }), NOW)).toBe('5 min ago: moved 3 files (5 MB)');
    expect(sweepText(sweep({ moved: 1, bytes: 1000, failed: 2 }), NOW)).toContain('2 stayed');
    expect(sweepText(sweep({ failed: 1, firstFailure: 'In use' }), NOW)).toBe('5 min ago: In use');
  });
});

describe('preview texts', () => {
  it('summarises what is due', () => {
    expect(dueSummary(plan(), 604_800)).toBe('Nothing is older than 7 days yet.');
    expect(dueSummary(plan({ dueTotal: 1, dueBytes: 2000 }), 600)).toBe('1 file (2 KB) is older than 10 minutes and would move to the 7-day quarantine.');
    expect(dueSummary(plan({ dueTotal: 4, dueBytes: 2000 }), 86_400)).toContain('4 files');
    expect(dueSummary(plan({ dueTotal: 4, dueBytes: 2000 }), 86_400)).toContain('are older');
  });
  it('says what stays and why', () => {
    expect(staysText(plan())).toBe('');
    const t = staysText(plan({ waiting: 2, other: 5, links: 1, refused: 1, dueTotal: 300, due: new Array(200).fill(null), truncated: true }));
    expect(t).toContain('2 matching files not old enough yet');
    expect(t).toContain('5 files left alone');
    expect(t).toContain('1 link skipped');
    expect(t).toContain('1 file refused by the safety rules');
    expect(t).toContain('oldest 200 of 300');
    expect(t).toContain('very large');
  });
  it('a due file becomes a table row', () => {
    const r = dueRow({ path: '/a/b/Xcode.dmg', bytes: 3 * 1000 * 1000, arrived: NOW - 3 * 86_400_000, modified: NOW - 3 * 86_400_000 }, NOW);
    expect(r).toEqual({ name: 'Xcode.dmg', path: '/a/b/Xcode.dmg', size: '3 MB', age: '3 days' });
  });
});
