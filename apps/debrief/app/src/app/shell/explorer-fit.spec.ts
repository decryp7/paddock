import { describe, expect, it } from 'vitest';
import { EXPLORER_WIDTH } from '../core/workspace/ui-prefs.store';
import { explorerAction, fitExplorer } from './explorer-fit';

const base = { open: true, chosen: false, wish: EXPLORER_WIDTH.default, hardRoom: 600, columnRoom: 250 };

describe('fitExplorer', () => {
  it('the default width yields to the other columns and lends the dock what it gave up', () => {
    expect(fitExplorer(base)).toEqual({ shown: 250, yield: 250 - EXPLORER_WIDTH.min });
  });

  it('a chosen width is shown within the whole room and lends the dock nothing', () => {
    expect(fitExplorer({ ...base, chosen: true, wish: 500 })).toEqual({ shown: 500, yield: 0 });
    expect(fitExplorer({ ...base, chosen: true, wish: 640 })).toEqual({ shown: 600, yield: 0 });
  });

  it('a chosen width equal to the default is still chosen', () => {
    expect(fitExplorer({ ...base, chosen: true, wish: EXPLORER_WIDTH.default })).toEqual({ shown: EXPLORER_WIDTH.default, yield: 0 });
  });

  it('a closed Explorer lends nothing', () => {
    expect(fitExplorer({ ...base, open: false }).yield).toBe(0);
  });

  it('never lends a negative amount', () => {
    expect(fitExplorer({ ...base, columnRoom: 100 }).yield).toBe(0);
  });
});

describe('explorerAction', () => {
  it('closes an open Explorer the plan has no room for, also one just opened by hand, and remembers it', () => {
    expect(explorerAction(true, 'hide', false)).toEqual({ action: 'close', closedByNarrowing: true });
    expect(explorerAction(true, 'hide', true)).toEqual({ action: 'close', closedByNarrowing: true });
  });

  it('keeps an Explorer that fits and forgets an earlier narrowing', () => {
    expect(explorerAction(true, 'keep', true)).toEqual({ action: 'none', closedByNarrowing: false });
  });

  it('reopens only an Explorer this follower closed, and only when there is room', () => {
    expect(explorerAction(false, 'show', true)).toEqual({ action: 'open', closedByNarrowing: false });
    expect(explorerAction(false, 'show', false)).toEqual({ action: 'none', closedByNarrowing: false });
    expect(explorerAction(false, 'keep', true)).toEqual({ action: 'none', closedByNarrowing: true });
  });
});
