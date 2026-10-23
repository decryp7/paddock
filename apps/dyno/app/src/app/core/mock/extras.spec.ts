import { MockWatch, mockHash, mockIsOs, mockSearchSeed, mockSearchUrl, mockStartupOverview } from './extras';
import { demoStartup } from './data';

const NOW = 1_700_000_000_000;

describe('mockSearchSeed', () => {
  it('removes the home folder, names, addresses and GUIDs and says what', () => {
    const s = mockSearchSeed('Slack at /Users/alex/Library/x for alex on alexs-macbook 10.0.0.5 {123e4567-e89b-12d3-a456-426614174000}');
    expect(s.text).toBe('Slack at Library/x for on');
    expect(s.removed).toEqual(['1 home folder', '1 user name', '1 computer name', '1 IP address', '1 GUID']);
    expect(mockSearchSeed('kernel_task').removed).toEqual([]);
    expect(mockSearchSeed('x '.repeat(300)).text.length).toBeLessThanOrEqual(200);
  });
});

describe('mockSearchUrl and mockHash', () => {
  it('encodes the text and checks it', () => {
    expect(mockSearchUrl(' a  b&c ')).toBe('https://duckduckgo.com/?q=a+b%26c');
    expect(() => mockSearchUrl('  ')).toThrow('Type something');
    expect(() => mockSearchUrl('x'.repeat(201))).toThrow('at most 200');
  });
  it('hashes are 64 hex digits and stable per path', () => {
    expect(mockHash('/a')).toMatch(/^[0-9a-f]{64}$/);
    expect(mockHash('/a')).toBe(mockHash('/a'));
    expect(mockHash('/a')).not.toBe(mockHash('/b'));
  });
});

describe('mockStartupOverview', () => {
  const all = demoStartup();
  it('counts do not depend on the filter, and each toggle hides its own kind', () => {
    const none = mockStartupOverview(all, { hideOs: false, hideSigned: false });
    expect(none.entries.length).toBe(all.length);
    expect(none.osCount).toBeGreaterThan(0);
    expect(none.signedCount).toBeGreaterThan(0);
    const os = mockStartupOverview(all, { hideOs: true, hideSigned: false });
    expect(os.entries.length).toBe(all.length - none.osCount);
    expect(os.osCount).toBe(none.osCount);
    const both = mockStartupOverview(all, { hideOs: true, hideSigned: true });
    expect(both.entries.length).toBe(all.length - none.osCount - none.signedCount);
    expect(both.entries.every((e) => !mockIsOs(e) && !e.publisher)).toBe(true);
  });
});

describe('MockWatch', () => {
  const make = () => new MockWatch(() => NOW);

  it('starts with three folders, two on', () => {
    const o = make().overview();
    expect(o.folders.map((f) => f.folder.enabled)).toEqual([true, true, false]);
    expect(o.templates).toHaveLength(3);
    expect(o.folders[2]!.nextSweep).toBeNull();
  });

  it('adds a folder switched off, refuses overlaps and bad times', () => {
    const w = make();
    expect(() => w.add(null, 5, false)).toThrow('between 10 minutes and 10 days');
    const o = w.add('build-output', 600, true);
    const added = o.folders.at(-1)!.folder;
    expect(added.path).toBe('/Users/alex/Movies/Exports');
    expect(added).toMatchObject({ enabled: false, previewed: false, recursive: true });
    w.add(null, 600, false);
    w.add(null, 600, false);
    expect(() => w.add(null, 600, false)).toThrow('already watched');
  });

  it('turning on needs the preview; a shorter time or subfolders turn it off again', () => {
    const w = make();
    expect(() => w.change('w-3', { enabled: true })).toThrow('preview first');
    w.preview('w-3');
    w.change('w-3', { enabled: true });
    expect(w.overview().folders[2]!.folder.enabled).toBe(true);
    w.change('w-3', { keepForSecs: 3 * 86400 });
    expect(w.overview().folders[2]!.folder.enabled).toBe(true);
    w.change('w-3', { keepForSecs: 86400 });
    expect(w.overview().folders[2]!.folder).toMatchObject({ enabled: false, previewed: false });
    w.change('w-1', { recursive: true });
    expect(w.overview().folders[0]!.folder).toMatchObject({ enabled: false, recursive: true });
    expect(() => w.change('w-1', { keepForSecs: 5 })).toThrow();
    expect(() => w.change('w-nope', {})).toThrow('no longer watches');
  });

  it('previews what is older than the keep time, sweeps only after a preview and removes', () => {
    const w = make();
    const p = w.preview('w-1');
    expect(p.plan.dueTotal).toBe(p.plan.due.length);
    expect(p.plan.due.every((d) => d.path.startsWith('/Users/alex/Downloads/'))).toBe(true);
    expect(p.plan.due.map((d) => d.arrived)).toEqual([...p.plan.due.map((d) => d.arrived)].sort((a, b) => a - b));
    const swept = w.sweep('w-1');
    expect(swept.folders[0]!.folder.lastSweep?.moved).toBe(p.plan.dueTotal);
    w.change('w-3', { keepForSecs: 86400, recursive: true });
    expect(() => w.sweep('w-3')).toThrow('preview first');
    expect(w.remove('w-3').folders).toHaveLength(2);
    expect(() => w.remove('w-3')).toThrow();
  });

  it('a template filter decides what is due', () => {
    const w = new MockWatch(() => NOW);
    const added = w.add('screenshots', 600, false).folders.at(-1)!.folder;
    expect(added.filter.nameStarts).toContain('Screenshot');
    // None of the demo files starts like a screenshot, so nothing in it is due.
    expect(w.preview(added.id).plan.dueTotal).toBe(0);
    expect(w.overview().folders.at(-1)!.filterText).toBe('Screenshot…, Screen Shot…, Screen Recording…, Capture…');
  });
});
