import { EMPTY_EXTRAS, ENVELOPE_VERSION, readLayout, sanitizeExtras, toEnvelope } from './layout-store';

const layout = { grid: { root: { type: 'leaf', data: { views: [], id: '1' } }, height: 1, width: 1, orientation: 'HORIZONTAL' }, panels: {} } as never;

describe('sanitizeExtras', () => {
  it('returns empty extras (a fresh copy) for non-objects', () => {
    for (const raw of [null, undefined, 'x', 3, [], [1]]) {
      const e = sanitizeExtras(raw);
      expect(e).toEqual(EMPTY_EXTRAS);
      expect(e).not.toBe(EMPTY_EXTRAS);
    }
    expect(sanitizeExtras({})).toEqual(EMPTY_EXTRAS);
  });

  it('keeps unique, non-empty string pinned tab ids only', () => {
    expect(sanitizeExtras({ pinnedTabs: ['a', 'a', '', 5, null, 'b'] }).pinnedTabs).toEqual(['a', 'b']);
    expect(sanitizeExtras({ pinnedTabs: 'a' }).pinnedTabs).toEqual([]);
  });

  it('sanitizes the auto-hidden items too', () => {
    const e = sanitizeExtras({ autoHidden: [{ id: 'a', side: 'left' }, { id: 'b', side: 'nope' }] });
    expect(e.autoHidden.map((i) => i.id)).toEqual(['a']);
  });
});

describe('sanitizeExtras: panels popped out into windows', () => {
  it('keeps unique, named pop-outs with a sanitized slot and drops the rest', () => {
    const e = sanitizeExtras({
      poppedOut: [{ id: 'a', slot: { referencePanel: 'b', direction: 'right' } }, { id: 'a' }, { id: '' }, 'x', { id: 'c', slot: 'nope' }, { id: 'd' }],
    });
    expect(e.poppedOut).toEqual([
      { id: 'a', slot: { referencePanel: 'b', direction: 'right' } },
      { id: 'c', slot: { referencePanel: null, direction: 'within' } },
      { id: 'd', slot: { referencePanel: null, direction: 'within' } },
    ]);
    expect(sanitizeExtras({ poppedOut: 'a' }).poppedOut).toBeUndefined();
    expect(sanitizeExtras({ poppedOut: [] })).toEqual(EMPTY_EXTRAS);
  });
});

describe('readLayout', () => {
  it('reads the current version with sanitized extras', () => {
    const saved = readLayout({ version: ENVELOPE_VERSION, layout, extras: { pinnedTabs: ['a', 'a'] } });
    expect(saved?.layout).toBe(layout);
    expect(saved?.extras).toEqual({ autoHidden: [], pinnedTabs: ['a'] });
    expect(readLayout({ version: ENVELOPE_VERSION, layout })?.extras).toEqual(EMPTY_EXTRAS);
  });

  it('is null for unknown versions and non-objects', () => {
    for (const version of [0, 2, '1', null, undefined]) expect(readLayout({ version, layout })).toBeNull();
    for (const raw of [null, undefined, 'x', 2, [], { version: ENVELOPE_VERSION }, { version: ENVELOPE_VERSION, layout: 'x' }, { version: ENVELOPE_VERSION, layout: [] }]) {
      expect(readLayout(raw)).toBeNull();
    }
  });
});

describe('toEnvelope', () => {
  it('stamps the current version and round-trips through readLayout', () => {
    const saved = { layout, extras: { autoHidden: [], pinnedTabs: ['a'] } };
    const env = toEnvelope(saved);
    expect(env).toEqual({ version: ENVELOPE_VERSION, layout, extras: saved.extras });
    expect(readLayout(JSON.parse(JSON.stringify(env)))).toEqual(saved);
  });
});

describe('sanitizeExtras auto-hide sides', () => {
  it('keeps the remembered side per pane, and loads layouts saved without it', () => {
    expect(sanitizeExtras({ autoHideSides: { inspector: 'bottom', x: 'top' } }).autoHideSides).toEqual({ inspector: 'bottom' });
    expect(sanitizeExtras({ autoHidden: [], pinnedTabs: [] }).autoHideSides).toBeUndefined();
  });
});

describe('sanitizeExtras: displaced views and user sizes (ADR 0056)', () => {
  it('keeps well-formed records, drops malformed and duplicate ones, and tolerates their absence', () => {
    const e = sanitizeExtras({
      displaced: [
        { id: 'home', slot: { referencePanel: null, direction: 'within', edge: 'right', size: 700 }, into: 'events' },
        { id: 'home', into: 'x' },
        { id: 'ai-chat', slot: 7, into: null },
        { slot: {} },
        'junk',
      ],
      sizes: { 'ai-chat': { width: 372, height: -4 }, bad: { width: 'x' }, worse: 5, home: { height: 600.4 } },
    });
    expect(e.displaced?.map((d) => d.id)).toEqual(['home', 'ai-chat']);
    expect(e.displaced?.[0]?.into).toBe('events');
    expect(e.displaced?.[0]?.slot).toEqual({ referencePanel: null, direction: 'within', edge: 'right', size: 700 });
    expect(e.displaced?.[1]?.into).toBeNull();
    expect(e.sizes).toEqual({ 'ai-chat': { width: 372 }, home: { height: 600.4 } });
    const none = sanitizeExtras({});
    expect(none.displaced).toBeUndefined();
    expect(none.sizes).toBeUndefined();
  });
});
