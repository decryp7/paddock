import type { Settings } from '../api.types';
import { makeMock, NOW } from '../../../testing/test-helpers';
import { MockPopover } from './mock-popover';

const kinds = (p: MockPopover) => p.view().layout.placements.map((x) => x.widget);

describe('MockPopover (the mock of the engine layout, for browser development and tests)', () => {
  it('starts with the defaults and reports them with their catalogue', () => {
    const p = new MockPopover();
    expect(kinds(p)).toEqual(['recentChanges', 'watchedSites', 'failures', 'nextCheck', 'notifications']);
    expect(p.view().catalogue.kinds).toHaveLength(6);
    expect(p.view().canUndo).toBe(false);
  });

  it('removes with one level of undo, which add, move and resize clear', () => {
    const p = new MockPopover();
    p.edit({ remove: { id: 'failures' } });
    expect(kinds(p)).not.toContain('failures');
    expect(p.view().canUndo).toBe(true);
    p.edit('undo');
    expect(kinds(p)).toContain('failures');
    expect(() => p.edit('undo')).toThrow();
    p.edit({ remove: { id: 'failures' } });
    p.edit({ add: { widget: 'quietHours', span: null, at: 0 } });
    expect(kinds(p)[0]).toBe('quietHours');
    expect(p.view().canUndo).toBe(false);
    p.edit({ move: { id: 'quiet-hours', to: 2 } });
    expect(kinds(p)[2]).toBe('quietHours');
    p.edit({ resize: { id: 'quiet-hours', span: 1 } });
    expect(p.view().layout.placements.find((x) => x.id === 'quiet-hours')?.span).toBe(1);
  });

  it('refuses what the engine would refuse', () => {
    const p = new MockPopover();
    expect(() => p.edit({ add: { widget: 'failures', span: null, at: null } })).toThrow();
    expect(() => p.edit({ remove: { id: 'nope' } })).toThrow();
    expect(() => p.edit({ move: { id: 'nope', to: 0 } })).toThrow();
    expect(() => p.edit({ resize: { id: 'failures', span: 1 } })).toThrow();
    expect(() => p.edit({ resize: { id: 'nope', span: 2 } })).toThrow();
    expect(() => p.edit({ savePreset: { name: 'x' } })).toThrow();
    expect(() => p.edit({ add: { widget: 'nope' as never, span: null, at: null } })).toThrow();
    p.edit({ add: { widget: 'quietHours', span: null, at: null } });
    expect(() => p.edit({ add: { widget: 'quietHours', span: null, at: null } })).toThrow();
    const full = new MockPopover();
    full.edit({ add: { widget: 'quietHours', span: null, at: null } });
    expect(kinds(full)).toHaveLength(6);
    full.edit({ remove: { id: 'failures' } });
    full.edit({ add: { widget: 'failures', span: null, at: null } });
    expect(() => full.edit({ add: { widget: 'nope' as never, span: null, at: null } })).toThrow();
  });

  it('resets to the defaults, with undo', () => {
    const p = new MockPopover();
    p.edit({ remove: { id: 'failures' } });
    p.edit('resetToDefaults');
    expect(kinds(p)).toContain('failures');
    expect(p.view().canUndo).toBe(true);
  });

  it('draws values from the watches: rows, empty states with a next step, paused notice', async () => {
    const watches = await makeMock().listWatches();
    const settings = await makeMock().getSettings();
    const p = new MockPopover();
    const values = p.values(watches, settings, NOW);
    expect(values.placements).toHaveLength(5);
    expect(values.notice).toBeNull();
    const none = p.values([], { ...settings, notificationsPausedUntilMs: NOW + 1000 }, NOW);
    expect(none.placements.every((x) => x.state.state === 'empty' || x.state.state === 'ready')).toBe(true);
    expect(none.notice).toBeNull();
    p.edit({ remove: { id: 'notifications' } });
    expect(p.values(watches, { ...settings, notificationsPausedUntilMs: NOW + 1000 } as Settings, NOW).notice?.title).toBe('Notifications are paused');
  });

  it('shows every kind, ready and empty', () => {
    const p = new MockPopover();
    for (const w of ['watchedSites', 'nextCheck', 'quietHours'] as const) {
      p.edit({ add: { widget: w, span: null, at: null } });
      p.edit({ remove: { id: p.view().layout.placements[0]!.id } });
    }
    const settings = { notifications: false, notificationsPausedUntilMs: null, quietHours: { start: '22:00', end: '07:00' } } as Settings;
    const v = p.values([], settings, NOW);
    expect(v.placements.length).toBeGreaterThan(0);
  });
});
