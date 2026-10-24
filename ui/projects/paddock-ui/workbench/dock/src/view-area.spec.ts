import { ANALYSIS_TAB_CAP, analysisTabToClose, viewArea } from './view-area';
import { viewLayout, viewRow } from './view-layout';

const declared = (area: 'document' | 'analysis' | 'side' | 'status') => ({ layout: { area, min: { width: 100, height: 100 }, pref: { width: 200, height: 200 }, priority: 1 } });

describe('viewArea', () => {
  it('is a document for a view that is not registered', () => {
    expect(viewArea(undefined)).toBe('document');
    expect(viewArea(null)).toBe('document');
  });

  it('is the area the view declares', () => {
    for (const area of ['document', 'analysis', 'side', 'status'] as const) expect(viewArea(declared(area))).toBe(area);
  });
});

describe('viewLayout and viewRow', () => {
  it('give the declared layout, else the conservative default of a document', () => {
    expect(viewLayout(declared('side')).area).toBe('side');
    expect(viewLayout(undefined).area).toBe('document');
  });

  it('puts analysis views in their own row and every other view in the main row', () => {
    expect(viewRow(declared('analysis'))).toBe('analysis');
    expect(viewRow(declared('document'))).toBe('main');
    expect(viewRow(declared('side'))).toBe('main');
  });
});

describe('analysisTabToClose', () => {
  const tabs = (n: number) => Array.from({ length: n }, (_, i) => `t${i}`);

  it('is null while the panel has room', () => {
    expect(analysisTabToClose(tabs(ANALYSIS_TAB_CAP - 1), 't0', new Set(), [])).toBeNull();
    expect(analysisTabToClose([], null, new Set(), [])).toBeNull();
  });

  it('closes the least recently used tab that is neither active nor pinned', () => {
    const all = tabs(ANALYSIS_TAB_CAP);
    // Most recent first: t0 is the newest, t7 the oldest.
    const mru = [...all];
    expect(analysisTabToClose(all, 't0', new Set(), mru)).toBe('t7');
    expect(analysisTabToClose(all, 't7', new Set(), mru)).toBe('t6');
    expect(analysisTabToClose(all, 't0', new Set(['t7', 't6']), mru)).toBe('t5');
  });

  it('treats tabs missing from the history as the oldest', () => {
    const all = tabs(ANALYSIS_TAB_CAP);
    expect(analysisTabToClose(all, 't0', new Set(), ['t7', 't6', 't5', 't4', 't3', 't2', 't1'])).toBe('t1');
    expect(analysisTabToClose(all, 't1', new Set(), all.slice(2))).toBe('t0');
  });

  it('is null when every tab is active or pinned', () => {
    const all = tabs(ANALYSIS_TAB_CAP);
    expect(analysisTabToClose(all, 't0', new Set(all.slice(1)), all)).toBeNull();
  });

  it('honours a custom cap', () => {
    expect(analysisTabToClose(['a', 'b', 'c'], 'a', new Set(), ['a', 'b', 'c'], 3)).toBe('c');
    expect(analysisTabToClose(['a', 'b'], 'a', new Set(), ['a', 'b'], 3)).toBeNull();
  });
});
