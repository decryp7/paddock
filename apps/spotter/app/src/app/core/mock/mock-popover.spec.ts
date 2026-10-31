import { NOW } from '../../../testing/test-helpers';
import { DEFAULT_KINDS, LOADING_SHAPE, MockPopover } from './mock-popover';

const kinds = (p: MockPopover) => p.view().layout.placements.map((x) => x.widget);

describe('MockPopover (the mock of the engine layout, for browser development and tests)', () => {
  it('starts with the defaults and reports them with their catalogue', () => {
    const p = new MockPopover();
    expect(kinds(p)).toEqual(DEFAULT_KINDS);
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
  });

  it('resets to the defaults, with undo', () => {
    const p = new MockPopover();
    p.edit({ remove: { id: 'failures' } });
    p.edit('resetToDefaults');
    expect(kinds(p)).toContain('failures');
    expect(p.view().canUndo).toBe(true);
  });

  it('serves what the engine produced for a scenario, for the placed kinds, with the paused notice only when no widget says it', () => {
    const p = new MockPopover();
    const demo = p.values('demo', NOW);
    expect(demo.placements.map((x) => x.id)).toEqual(p.view().layout.placements.map((x) => x.id));
    expect(demo.notice).toBeNull();
    const worst = p.values('worst', NOW);
    expect(worst.notice?.title).toBe('Notifications are paused');
    p.edit({ add: { widget: 'notifications', span: null, at: null } });
    expect(p.values('worst', NOW).notice).toBeNull();
    const empty = new MockPopover().values('empty', NOW).placements.map((x) => x.state.state);
    expect(empty.every((s) => s === 'empty')).toBe(true);
  });

  it('serves the engine\'s overall status for a scenario, so the header pill and the widget heads agree', () => {
    const p = new MockPopover();
    expect(p.status('worst')).toEqual({ kind: 'failing', count: 4 });
    expect(p.status('empty')).toEqual({ kind: 'unknown' });
    expect(p.status('demo').kind).toBe('failing');
    expect(p.status('unchecked')).toEqual({ kind: 'unchecked', count: 2 });
  });

  it('moves the engine\'s times to the mock\'s now', () => {
    const state = (now: number) => new MockPopover(['nextCheck']).values('demo', now).placements[0]!.state;
    const at = (now: number) => (state(now).state === 'ready' ? (state(now) as { value: { atMs: number } }).value.atMs : 0);
    expect(at(NOW + 5000) - at(NOW)).toBe(5000);
  });

  it('reads the catalogue, the defaults and the loading shape from the engine\'s fixtures', () => {
    expect(DEFAULT_KINDS).toEqual(['recentChanges', 'failures', 'nextCheck']);
    expect(LOADING_SHAPE.map((l) => [l.kind, l.rows])).toEqual([
      ['watchedSites', 4],
      ['recentChanges', 1],
      ['nextCheck', 0],
      ['quietHours', 0],
      ['failures', 1],
      ['notifications', 0],
    ]);
  });
});
