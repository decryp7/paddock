import { PANEL_LIMITS, type PanelsEvent } from '../generated/panels';
import { hostOf, InPageDesktop, MockGhost, NO_GHOST, recipients, validPanel } from './in-page';

describe('validPanel', () => {
  it('accepts a namespaced view and an instance key of 1 to 1024 characters', () => {
    for (const ok of ['pn.code', 'pn.code#src/main.rs', 'pn.code#a#b', `pn.code#${'k'.repeat(PANEL_LIMITS.maxKey)}`]) expect(validPanel(ok)).toBe(true);
  });

  it.each(['', 'code', 'Pn.code', 'pn.code#', '#key', 'pn.code#a\nb', `pn.code#${'k'.repeat(PANEL_LIMITS.maxKey + 1)}`, `pn.${'x'.repeat(70)}`])('refuses %j', (id) => {
    expect(validPanel(id)).toBe(false);
  });
});

describe('recipients (mirrors paddock_panels::sync::recipients)', () => {
  const all = ['main', 'w-2', 'p-main-1', 'p-main-2', 'p-w-2-3', 'd-main-4', 'd-w-2-5', 'tray', 'x'];
  it('full reaches the host and its pop-outs only; display also its dialogs; never another host', () => {
    expect(recipients('main', 'full', all)).toEqual(['p-main-1', 'p-main-2']);
    expect(recipients('p-main-1', 'full', all)).toEqual(['main', 'p-main-2']);
    expect(recipients('main', 'display', all)).toEqual(['d-main-4', 'p-main-1', 'p-main-2']);
    expect(recipients('d-w-2-5', 'full', all)).toEqual(['p-w-2-3', 'w-2']);
    expect(recipients('tray', 'display', all)).toEqual([]);
    expect(recipients('x', 'full', all)).toEqual([]);
    expect(hostOf('p-w-2-3')).toBe('w-2');
    expect(hostOf('w-2')).toBe('w-2');
    expect(hostOf('tray')).toBeNull();
  });
});

describe('InPageDesktop', () => {
  it('pops panels out as windows of the host, focuses the one that exists and limits them', async () => {
    const desktop = new InPageDesktop();
    const main = desktop.window('main');
    const events: PanelsEvent[] = [];
    await main.onPanels((e) => events.push(e));
    const first = await main.popOut({ panel: 'pn.code', title: ' Code ' });
    expect(first).toMatchObject({ label: 'p-main-1', host: 'main', panel: 'pn.code', title: 'Code', keepOnTop: false });
    expect((await main.popOut({ panel: 'pn.code' })).label).toBe('p-main-1');
    expect(events).toEqual([{ kind: 'changed' }]);
    expect(await main.focus('pn.code')).toBe(true);
    expect(await main.focus('pn.log')).toBe(false);
    expect(main.focusLog).toEqual(['pn.code']);
    expect(await main.list()).toHaveLength(1);
    expect(await desktop.window('p-main-1').list()).toHaveLength(1);
    expect(await desktop.window('w-2').list()).toEqual([]);
    for (let i = 1; i < PANEL_LIMITS.maxPopOuts; i++) await main.popOut({ panel: `pn.v${i}` });
    await expect(main.popOut({ panel: 'pn.over' })).rejects.toThrow(/At most 12/);
    await expect(main.popOut({ panel: '../x' })).rejects.toThrow(/not a panel id/);
    await expect(desktop.window('p-main-1').popOut({ panel: 'pn.deep' })).rejects.toThrow(/application window/);
  });

  it('docks back by panel or from the pop-out itself, and reports a closed window to its host', async () => {
    const desktop = new InPageDesktop();
    const main = desktop.window('main');
    const events: PanelsEvent[] = [];
    await main.onPanels((e) => events.push(e));
    await main.popOut({ panel: 'pn.code' });
    await main.popOut({ panel: 'pn.log' });
    await main.dockBack('pn.code');
    await desktop.window('p-main-2').dockBack();
    expect(events.filter((e) => e.kind === 'returned')).toEqual([
      { kind: 'returned', panel: 'pn.code', label: 'p-main-1' },
      { kind: 'returned', panel: 'pn.log', label: 'p-main-2' },
    ]);
    await expect(main.dockBack()).rejects.toThrow(/name the panel/);
    await expect(main.dockBack('pn.nothing')).rejects.toThrow(/not popped out/);
    await main.popOut({ panel: 'pn.again' });
    desktop.closeWindow('p-main-3');
    desktop.closeWindow('p-main-3');
    expect(desktop.all()).toEqual([]);
  });

  it('keep on top applies to a pop-out; a drag and a closing host are reported or silent as Rust does', async () => {
    const desktop = new InPageDesktop();
    const main = desktop.window('main');
    const events: PanelsEvent[] = [];
    await main.onPanels((e) => events.push(e));
    await main.popOut({ panel: 'pn.code' });
    await desktop.window('p-main-1').keepOnTop(true);
    expect(desktop.all()[0]?.keepOnTop).toBe(true);
    await expect(main.keepOnTop(true)).rejects.toThrow(/pop-out/);
    desktop.dragWindow('p-main-1', 5, 6);
    desktop.dragWindow('p-main-9', 5, 6);
    expect(events.at(-1)).toEqual({ kind: 'moved', panel: 'pn.code', label: 'p-main-1', x: 5, y: 6 });
    const before = events.length;
    desktop.closeHost('main');
    expect(desktop.all()).toEqual([]);
    expect(events).toHaveLength(before);
    expect(await desktop.window('p-main-1').pointerDown()).toBe(false);
  });

  it('relays UI state to the right windows and refuses what Rust refuses', async () => {
    const desktop = new InPageDesktop();
    const main = desktop.window('main');
    const got: Record<string, unknown[]> = { 'p-main-1': [], 'd-main-3': [], 'p-w-2-2': [] };
    await main.popOut({ panel: 'pn.code' });
    await desktop.window('w-2').popOut({ panel: 'pn.code' });
    for (const l of Object.keys(got)) await desktop.window(l).onSync((m) => got[l]?.push(m));
    expect(await main.publish({ scope: 'full', message: { t: 'x', n: 1 } })).toBe(1);
    expect(await main.publish({ scope: 'display', message: { t: 'y' } })).toBe(2);
    expect(got).toEqual({ 'p-main-1': [{ t: 'x', n: 1 }, { t: 'y' }], 'd-main-3': [{ t: 'y' }], 'p-w-2-2': [] });
    await expect(main.publish({ scope: 'full', message: [1] })).rejects.toThrow();
    await expect(main.publish({ scope: 'full', message: { v: 'x'.repeat(PANEL_LIMITS.maxMessageBytes) } })).rejects.toThrow();
    expect(desktop.refused).toBe(2);
    const off = await main.onSync(() => undefined);
    off();
  });
});

describe('MockGhost', () => {
  it('follows the pointer while shown and answers where it was on end', async () => {
    const ghost = new MockGhost();
    await ghost.show(true);
    expect(ghost.state.visible).toBe(false);
    expect(await ghost.start({ title: 'T', width: 400, height: 300, grabX: 100, grabY: 10, snapshot: 'data:image/png;base64,AA==' })).toBe(true);
    expect(ghost.state.request).toMatchObject({ title: 'T', hasSnapshot: true });
    ghost.pointerAt(500, 500);
    await ghost.show(true);
    ghost.pointerAt(800, 600);
    expect(ghost.state.rect).toEqual({ x: 700, y: 590, width: 400, height: 300, scale: 1 });
    expect(await ghost.end()).toEqual({ x: 700, y: 590, width: 400, height: 300, scale: 1 });
    expect(ghost.state).toMatchObject({ starts: 1, ends: 1, visible: false });
    await ghost.start({ title: 'T', width: 1, height: 1, grabX: 0, grabY: 0 });
    expect(await ghost.end()).toBeNull();
    expect(ghost.available).toBe(true);
  });

  it('is absent in a browser', async () => {
    expect(NO_GHOST.available).toBe(false);
    expect(await NO_GHOST.start({ title: 'T', width: 1, height: 1, grabX: 0, grabY: 0 })).toBe(false);
    await NO_GHOST.show(true);
    expect(await NO_GHOST.end()).toBeNull();
  });
});
