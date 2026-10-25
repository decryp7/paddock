import { EMPTY_EXTRAS, SavedLayout, toEnvelope } from './layout-store';
import {
  cleanLayoutName,
  layoutNameProblem,
  MAX_LAYOUT_NAME,
  MAX_NAMED_LAYOUTS,
  moveLayout,
  NamedLayout,
  NamedLayouts,
  parseNamedLayouts,
  removeLayout,
  renameLayout,
  serializeNamedLayouts,
  upsertLayout,
} from './named-layouts';
import { at } from './must.spec-helper';
import { type DockLayoutStore, InMemoryDockLayoutStore } from './dock-store';

const grid = { grid: { root: { type: 'leaf', data: { views: [], id: '1' } }, height: 1, width: 1, orientation: 'HORIZONTAL' }, panels: {} } as never;
const saved = (): SavedLayout => ({ layout: grid, extras: { ...EMPTY_EXTRAS } });
const nl = (name: string, savedAt = 1): NamedLayout => ({ name, savedAt, saved: saved() });
const names = (l: readonly NamedLayout[]) => l.map((x) => x.name);
const entry = (name: unknown, over: Record<string, unknown> = {}) => ({ name, savedAt: 5, envelope: toEnvelope(saved()), ...over });

describe('cleanLayoutName', () => {
  it('trims and replaces control characters with spaces', () => {
    expect(cleanLayoutName('  My layout  ')).toBe('My layout');
    expect(cleanLayoutName('a\nb\tc\u007fd')).toBe('a b c d');
    expect(cleanLayoutName('\n\u0000x\n')).toBe('x');
  });

  it('truncates to 60 characters and trims again', () => {
    expect(cleanLayoutName('x'.repeat(100))).toBe('x'.repeat(MAX_LAYOUT_NAME));
    expect(cleanLayoutName('x'.repeat(59) + '   tail')).toBe('x'.repeat(59));
  });

  it('is null when nothing is left', () => {
    expect(cleanLayoutName('')).toBeNull();
    expect(cleanLayoutName(' \n\t ')).toBeNull();
  });
});

describe('layoutNameProblem', () => {
  it('refuses an empty name, one that is too long as typed, and one another layout has (any case)', () => {
    expect(layoutNameProblem('', [])).toBe('empty');
    expect(layoutNameProblem(' \n\u0000 ', [])).toBe('empty');
    expect(layoutNameProblem('x'.repeat(MAX_LAYOUT_NAME + 1), [])).toBe('too-long');
    expect(layoutNameProblem('x'.repeat(MAX_LAYOUT_NAME), [])).toBeNull();
    expect(layoutNameProblem('  Work ', ['work'])).toBe('duplicate');
    expect(layoutNameProblem('Play', ['work'])).toBeNull();
  });

  it('lets a layout being renamed keep (or change the case of) its own name', () => {
    expect(layoutNameProblem('Work', ['Work', 'Play'], 'Work')).toBeNull();
    expect(layoutNameProblem('Play', ['Work', 'Play'], 'Work')).toBe('duplicate');
  });
});

describe('upsertLayout', () => {
  it('appends new names', () => {
    expect(names(upsertLayout([nl('a')], nl('b')))).toEqual(['a', 'b']);
  });

  it('replaces a same name case-insensitively, in place', () => {
    const next = upsertLayout([nl('Work', 1), nl('b', 2)], nl('WORK', 9));
    expect(next.map((l) => [l.name, l.savedAt])).toEqual([
      ['WORK', 9],
      ['b', 2],
    ]);
  });

  it('drops the oldest past the cap', () => {
    const full = Array.from({ length: MAX_NAMED_LAYOUTS }, (_, i) => nl(`l${i}`));
    const next = upsertLayout(full, nl('new'));
    expect(next).toHaveLength(MAX_NAMED_LAYOUTS);
    expect(at(next, 0).name).toBe('l1');
    expect(at(next, next.length - 1).name).toBe('new');
    expect(upsertLayout(full, nl('L0'))).toHaveLength(MAX_NAMED_LAYOUTS);
  });

  it('does not mutate the input', () => {
    const list = [nl('a')];
    upsertLayout(list, nl('b'));
    expect(list).toHaveLength(1);
  });
});

describe('removeLayout', () => {
  it('removes by exact name', () => {
    expect(names(removeLayout([nl('a'), nl('b')], 'a'))).toEqual(['b']);
    expect(names(removeLayout([nl('a')], 'A'))).toEqual(['a']);
    expect(removeLayout([], 'a')).toEqual([]);
  });
});

describe('renameLayout', () => {
  const list = [nl('a'), nl('b')];

  it('renames and cleans the new name', () => {
    expect(names(renameLayout(list, 'a', '  c\n'))).toEqual(['c', 'b']);
  });

  it('refuses empty names and names taken by another layout, case-insensitively', () => {
    expect(names(renameLayout(list, 'a', '  '))).toEqual(['a', 'b']);
    expect(names(renameLayout(list, 'a', 'B'))).toEqual(['a', 'b']);
  });

  it('allows changing only the case of its own name', () => {
    expect(names(renameLayout(list, 'a', 'A'))).toEqual(['A', 'b']);
  });

  it('returns a copy and keeps the list for an unknown source', () => {
    const same = renameLayout(list, 'zzz', 'q');
    expect(names(same)).toEqual(['a', 'b']);
    expect(same).not.toBe(list);
  });
});

describe('moveLayout', () => {
  const list = [nl('a'), nl('b'), nl('c')];

  it('swaps with the neighbour', () => {
    expect(names(moveLayout(list, 'b', -1))).toEqual(['b', 'a', 'c']);
    expect(names(moveLayout(list, 'b', 1))).toEqual(['a', 'c', 'b']);
  });

  it('does nothing at the bounds or for unknown names', () => {
    expect(names(moveLayout(list, 'a', -1))).toEqual(['a', 'b', 'c']);
    expect(names(moveLayout(list, 'c', 1))).toEqual(['a', 'b', 'c']);
    expect(names(moveLayout(list, 'zzz', 1))).toEqual(['a', 'b', 'c']);
    expect(names(moveLayout(list, 'zzz', -1))).toEqual(['a', 'b', 'c']);
  });
});

describe('parseNamedLayouts', () => {
  const wrap = (layouts: unknown, version: unknown = 1) => ({ version, layouts });

  it('is empty for the wrong version or shape', () => {
    for (const raw of [null, undefined, 'x', 3, {}, wrap([entry('a')], 2), wrap([entry('a')], null), wrap('nope'), wrap({})]) {
      expect(parseNamedLayouts(raw)).toEqual([]);
    }
  });

  it('drops malformed entries', () => {
    const out = parseNamedLayouts(
      wrap([null, 'x', entry(5), entry('   '), entry('nover', { envelope: { layout: grid } }), entry('badenv', { envelope: null }), entry('good')]),
    );
    expect(names(out)).toEqual(['good']);
  });

  it('drops duplicate names case-insensitively, keeping the first', () => {
    expect(names(parseNamedLayouts(wrap([entry('Dup', { savedAt: 1 }), entry('dup', { savedAt: 2 }), entry('x')])))).toEqual(['Dup', 'x']);
  });

  it('cleans names and defaults a missing savedAt to 0', () => {
    const l = at(parseNamedLayouts(wrap([entry(' a\nb ', { savedAt: 'yesterday' })])), 0);
    expect(l.name).toBe('a b');
    expect(l.savedAt).toBe(0);
  });

  it('keeps at most the cap', () => {
    const many = Array.from({ length: MAX_NAMED_LAYOUTS + 5 }, (_, i) => entry(`l${i}`));
    expect(parseNamedLayouts(wrap(many))).toHaveLength(MAX_NAMED_LAYOUTS);
  });

  it('round-trips through serialize', () => {
    const list = [{ ...nl('a', 10), saved: { layout: grid, extras: { ...EMPTY_EXTRAS, pinnedTabs: ['p'] } } }, nl('b', 20)];
    expect(parseNamedLayouts(JSON.parse(JSON.stringify(serializeNamedLayouts(list))))).toEqual(list);
  });
});

describe('NamedLayouts', () => {
  const KEY = 'wb.layouts';
  const make = async (initial: NamedLayout[] = [], store = new InMemoryDockLayoutStore()) => {
    if (initial.length) await store.save(KEY, serializeNamedLayouts(initial));
    const layouts = new NamedLayouts(store, KEY);
    await layouts.ready;
    return { layouts, store };
  };
  const stored = async (store: InMemoryDockLayoutStore) => parseNamedLayouts(await store.load(KEY));

  it('starts from the stored list', async () => {
    expect(names((await make([nl('a')])).layouts.list())).toEqual(['a']);
  });

  it('starts empty from corrupt or foreign data and from a store that fails', async () => {
    const store = new InMemoryDockLayoutStore();
    store.entries.set(KEY, '{"version":9,"layouts":[]}');
    expect((await make([], store)).layouts.list()).toEqual([]);
    const failing: DockLayoutStore = { load: () => Promise.reject(new Error('away')), save: () => Promise.reject(new Error('away')) };
    const layouts = new NamedLayouts(failing, KEY);
    await layouts.ready;
    expect(layouts.list()).toEqual([]);
    expect(layouts.save('a', saved(), 1)).toBe('a');
  });

  it('keeps a layout saved before the stored list was read', async () => {
    const store = new InMemoryDockLayoutStore();
    await store.save(KEY, serializeNamedLayouts([nl('old')]));
    const layouts = new NamedLayouts(store, KEY);
    layouts.save('new', saved(), 3);
    await layouts.ready;
    expect(names(layouts.list())).toEqual(['old', 'new']);
  });

  it('saves under the cleaned name and persists', async () => {
    const { layouts, store } = await make();
    expect(layouts.save('  Debug\n', saved(), 42)).toBe('Debug');
    expect(layouts.list()[0]).toMatchObject({ name: 'Debug', savedAt: 42 });
    expect(names(await stored(store))).toEqual(['Debug']);
  });

  it('refuses an empty name without touching the store', async () => {
    const { layouts, store } = await make();
    expect(layouts.save('  ', saved())).toBeNull();
    expect(layouts.list()).toEqual([]);
    expect(store.entries.size).toBe(0);
  });

  it('saving the same name again replaces it', async () => {
    const { layouts } = await make();
    layouts.save('a', saved(), 1);
    layouts.save('A', saved(), 2);
    expect(layouts.list().map((l) => [l.name, l.savedAt])).toEqual([['A', 2]]);
  });

  it('renames, reporting whether anything changed', async () => {
    const { layouts, store } = await make([nl('a'), nl('b')]);
    expect(layouts.rename('a', 'c')).toBe(true);
    expect(names(layouts.list())).toEqual(['c', 'b']);
    expect(names(await stored(store))).toEqual(['c', 'b']);
    expect(layouts.rename('c', 'b')).toBe(false);
    expect(layouts.rename('c', ' ')).toBe(false);
    expect(layouts.rename('c', 'c')).toBe(false);
    expect(layouts.rename('zzz', 'q')).toBe(false);
  });

  it('reads the stored list again when another window changed it, and keeps the list when the store is away', async () => {
    const { layouts, store } = await make([nl('a')]);
    await store.save(KEY, serializeNamedLayouts([nl('x'), nl('y')]));
    await layouts.reload();
    expect(names(layouts.list())).toEqual(['x', 'y']);
    const broken: DockLayoutStore = { load: () => Promise.reject(new Error('away')), save: () => Promise.resolve() };
    const kept = new NamedLayouts(broken, KEY);
    kept.list.set([nl('keep')]);
    await kept.reload();
    expect(names(kept.list())).toEqual(['keep']);
  });

  it('moves, removes, finds and indexes layouts', async () => {
    const { layouts, store } = await make([nl('a'), nl('b'), nl('c')]);
    layouts.move('c', -1);
    expect(names(layouts.list())).toEqual(['a', 'c', 'b']);
    expect(layouts.at(1)?.name).toBe('a');
    expect(layouts.at(3)?.name).toBe('b');
    expect(layouts.at(0)).toBeUndefined();
    expect(layouts.at(4)).toBeUndefined();
    expect(layouts.find('c')?.name).toBe('c');
    expect(layouts.find('zzz')).toBeUndefined();
    layouts.remove('a');
    expect(names(layouts.list())).toEqual(['c', 'b']);
    expect(names(await stored(store))).toEqual(['c', 'b']);
  });
});
