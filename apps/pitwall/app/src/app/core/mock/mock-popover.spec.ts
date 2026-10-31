import { DEFAULTS, DEFAULT_KINDS, LOADING_SHAPE, MockPopover } from './mock-popover';

const NOW = 1_900_000_000_000;

describe('the mock popup (the engine\'s recorded output)', () => {
  it('starts with the engine\'s default layout and loading shape', () => {
    const view = new MockPopover().view();
    expect(DEFAULTS).toEqual([[1, 'server'], [1, 'activity'], [2, 'memory'], [2, 'loadedModels']]);
    expect(view.layout.placements.map((p) => [p.span, p.widget])).toEqual(DEFAULTS);
    expect(view.catalogue.kinds.length).toBe(7);
    expect(view.catalogue.surface.maxPlacements).toBe(7);
    expect(LOADING_SHAPE.filter((s) => s.rows > 0).map((s) => [s.kind, s.rows])).toEqual([['loadedModels', 2], ['downloads', 2], ['recentErrors', 2]]);
  });

  it('serves each scenario\'s states and header, with times moved to the mock\'s now', () => {
    const popup = new MockPopover([...DEFAULT_KINDS, 'downloads', 'updates', 'recentErrors']);
    expect(popup.header('demo').status).toEqual({ kind: 'serving', port: 11435 });
    expect(popup.header('worst').status).toEqual({ kind: 'problems', count: 1 });
    expect(popup.header('down').status).toEqual({ kind: 'unreachable' });
    expect(popup.header('paused').serving).toBe('resume');
    const demo = popup.values('demo', NOW);
    expect(demo.notice).toBeNull();
    expect(demo.placements.map((p) => p.state.state)).toEqual(['ready', 'ready', 'ready', 'ready', 'working', 'ready', 'ready']);
    const down = popup.values('down', NOW);
    expect(down.notice?.title).toBe('The server is not answering');
    const server = down.placements[0]!.state;
    expect(server.state === 'ready' && server.staleSince).toBe(NOW);
    const errors = popup.values('demo', NOW).placements.find((p) => p.id === 'recent-errors')!.state;
    expect(errors.state === 'ready' && errors.value.kind === 'recentErrors' && errors.value.rows[0]!.atMs).toBe(NOW - 12 * 60_000);
  });

  it('does the chooser\'s edits and refuses what the engine would', () => {
    const popup = new MockPopover();
    const ids = () => popup.view().layout.placements.map((p) => p.id);
    popup.edit({ remove: { id: 'memory' } });
    expect(ids()).toEqual(['server', 'activity', 'loaded-models']);
    expect(popup.view().canUndo).toBe(true);
    popup.edit('undo');
    expect(ids().length).toBe(4);
    popup.edit({ move: { id: 'loaded-models', to: 0 } });
    expect(ids()[0]).toBe('loaded-models');
    popup.edit({ resize: { id: 'server', span: 2 } });
    expect(popup.view().layout.placements.find((p) => p.id === 'server')!.span).toBe(2);
    expect(() => popup.edit({ resize: { id: 'memory', span: 1 } })).toThrow();
    expect(() => popup.edit({ add: { widget: 'memory', span: null, at: null } })).toThrow();
    popup.edit({ add: { widget: 'downloads', span: null, at: null } });
    popup.edit({ add: { widget: 'updates', span: null, at: 1 } });
    popup.edit({ add: { widget: 'recentErrors', span: null, at: null } });
    expect(ids().length).toBe(7);
    expect(() => popup.edit('resetToDefaults')).not.toThrow();
    expect(ids().length).toBe(4);
    expect(() => popup.edit({ remove: { id: 'nope' } })).toThrow();
    expect(() => popup.edit({ move: { id: 'nope', to: 0 } })).toThrow();
  });
});
