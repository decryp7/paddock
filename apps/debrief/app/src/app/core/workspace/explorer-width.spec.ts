import { clampExplorerWidth, EXPLORER_WIDTH, sanitizePrefs } from './ui-prefs.store';
import { EXPLORER_LAYOUT } from '../../dock/dock-layout';

// The Explorer's width is a declared range (ADR 0056): the Explorer's ViewLayout and the stored preference agree on it.
describe('Explorer width', () => {
  it('is declared as a range the plan can move within, not a fixed width', () => {
    expect(EXPLORER_LAYOUT.min.width).toBe(EXPLORER_WIDTH.min);
    expect(EXPLORER_LAYOUT.pref.width).toBe(EXPLORER_WIDTH.default);
    expect(EXPLORER_WIDTH.min).toBeLessThan(EXPLORER_WIDTH.max);
    expect(EXPLORER_WIDTH.default).toBeGreaterThan(EXPLORER_WIDTH.min);
  });

  it('is clamped to the declared limits', () => {
    expect(clampExplorerWidth(10)).toBe(EXPLORER_WIDTH.min);
    expect(clampExplorerWidth(5000)).toBe(EXPLORER_WIDTH.max);
    expect(clampExplorerWidth(301.4)).toBe(301);
  });

  it('falls back to the default for anything that is not a finite number', () => {
    for (const bad of [undefined, null, 'wide', NaN, Infinity, {}]) expect(clampExplorerWidth(bad)).toBe(EXPLORER_WIDTH.default);
  });

  it('is read from stored preferences once, clamped, and defaults when missing or corrupt', () => {
    expect(sanitizePrefs({ explorerWidth: 400 }).explorerWidth).toBe(400);
    expect(sanitizePrefs({ explorerWidth: 99999 }).explorerWidth).toBe(EXPLORER_WIDTH.max);
    expect(sanitizePrefs({}).explorerWidth).toBe(EXPLORER_WIDTH.default);
    expect(sanitizePrefs({ explorerWidth: 'x' }).explorerWidth).toBe(EXPLORER_WIDTH.default);
  });
});
