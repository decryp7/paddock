import { PANEL_EVENTS } from '../generated/panels';
import { parsePanelsEvent, tauriPanelsIo, type PanelsBridge } from './io';

function bridge() {
  const calls: { command: string; args?: Record<string, unknown> }[] = [];
  const listeners = new Map<string, (e: { payload: unknown }) => void>();
  const b: PanelsBridge = {
    invoke: <T>(command: string, args?: Record<string, unknown>) => {
      calls.push({ command, ...(args ? { args } : {}) });
      return Promise.resolve(null as T);
    },
    listen: async (event, handler) => {
      listeners.set(event, handler);
      return () => void listeners.delete(event);
    },
    label: () => 'main',
  };
  return { b, calls, listeners };
}

describe('parsePanelsEvent', () => {
  it('parses each kind of event', () => {
    expect(parsePanelsEvent({ kind: 'returned', panel: 'pn.code', label: 'p-main-1' })).toEqual({ kind: 'returned', panel: 'pn.code', label: 'p-main-1' });
    expect(parsePanelsEvent({ kind: 'changed', extra: 1 })).toEqual({ kind: 'changed' });
    expect(parsePanelsEvent({ kind: 'moved', panel: 'pn.code', label: 'p-main-1', x: 1, y: 2 })).toEqual({ kind: 'moved', panel: 'pn.code', label: 'p-main-1', x: 1, y: 2 });
    for (const look of ['opaque', 'translucent', 'outline'] as const) {
      expect(parsePanelsEvent({ kind: 'redock', panel: 'pn.code', label: 'p-main-1', look })).toEqual({ kind: 'redock', panel: 'pn.code', label: 'p-main-1', look });
    }
  });

  it.each([null, undefined, 'x', 5, [], {}, { kind: 'other' }, { kind: 'returned', panel: 'a.b' }, { kind: 'moved', panel: 'a.b', label: 'l', x: 'a', y: 1 }, { kind: 'moved', panel: '', label: 'l', x: 1, y: 1 }, { kind: 'redock', panel: 'a.b', label: 'l', look: 'glass' }, { kind: 'redock', panel: 'a.b', label: 'l' }])('refuses %j', (raw) => {
    expect(parsePanelsEvent(raw)).toBeNull();
  });
});

describe('tauriPanelsIo', () => {
  it('sends each call as its command with the argument names Rust expects', async () => {
    const { b, calls } = bridge();
    const io = tauriPanelsIo(b);
    expect(await io.label()).toBe('main');
    await io.popOut({ panel: 'pn.code' });
    await io.dockBack();
    await io.dockBack('pn.code');
    await io.list();
    await io.focus('pn.code');
    await io.keepOnTop(true);
    await io.publish({ scope: 'full', message: { t: 'bye' } });
    await io.pointerDown();
    expect(io.ghost.available).toBe(true);
    await io.ghost.start({ title: 'T', width: 1, height: 1, grabX: 0, grabY: 0 });
    await io.ghost.show(true);
    await io.ghost.end();
    expect(calls).toEqual([
      { command: 'panel_pop_out', args: { request: { panel: 'pn.code' } } },
      { command: 'panel_dock_back', args: { panel: null } },
      { command: 'panel_dock_back', args: { panel: 'pn.code' } },
      { command: 'panels_list' },
      { command: 'panel_focus', args: { panel: 'pn.code' } },
      { command: 'panel_keep_on_top', args: { on: true } },
      { command: 'ui_sync_publish', args: { sync: { scope: 'full', message: { t: 'bye' } } } },
      { command: 'pointer_button_down' },
      { command: 'drag_ghost_start', args: { request: { title: 'T', width: 1, height: 1, grabX: 0, grabY: 0 } } },
      { command: 'drag_ghost_show', args: { visible: true } },
      { command: 'drag_ghost_end' },
    ]);
  });

  it('listens to the layer events, parsing the panels one at the edge', async () => {
    const { b, listeners } = bridge();
    const io = tauriPanelsIo(b);
    const events: unknown[] = [];
    const syncs: unknown[] = [];
    const offPanels = await io.onPanels((e) => events.push(e));
    const offSync = await io.onSync((m) => syncs.push(m));
    listeners.get(PANEL_EVENTS.panels)?.({ payload: { kind: 'changed' } });
    listeners.get(PANEL_EVENTS.panels)?.({ payload: { kind: 'nonsense' } });
    listeners.get(PANEL_EVENTS.uiSync)?.({ payload: { t: 'bye' } });
    expect(events).toEqual([{ kind: 'changed' }]);
    expect(syncs).toEqual([{ t: 'bye' }]);
    offPanels();
    offSync();
    expect(listeners.size).toBe(0);
  });
});
