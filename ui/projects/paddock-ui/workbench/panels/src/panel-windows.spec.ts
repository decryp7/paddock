import { TestBed } from '@angular/core/testing';
import { settle, shellPorts } from '@decryp7/paddock-ui/shell/testing';
import { ShellContextHost } from '@decryp7/paddock-ui/shell';
import { PANEL_WINDOWS, WORKBENCH_PORTS, type WorkbenchPorts } from '@decryp7/paddock-ui/workbench';
import { PANEL_LIMITS } from '../generated/panels';
import { InPageDesktop, MockGhost } from './in-page';
import { HOST_TITLE_KEY, PanelWindows, parseDragOver, type PopOutMoved } from './panel-windows';
import { providePanelWindows } from './provide';
import { FakeSyncTimers, newCtx, windowProviders, type TestCtx } from './testing';
import { SYNC_TIMERS, syncedValues, UiSync } from './ui-sync';
import { signal } from '@angular/core';
import { WorkbenchRegistry } from '@decryp7/paddock-ui/workbench';

function setup(options: { ghost?: MockGhost; hostCommands?: (id: string) => boolean; title?: string } = {}) {
  const desktop = new InPageDesktop('main', options.ghost);
  const ctx: TestCtx = newCtx();
  const shown: string[] = [];
  const ports = { showView: (id: string) => (shown.push(id), Promise.resolve(true)) } as unknown as WorkbenchPorts;
  const timers = new FakeSyncTimers();
  TestBed.resetTestingModule();
  TestBed.configureTestingModule({
    providers: [
      ...windowProviders(ctx),
      { provide: SYNC_TIMERS, useValue: timers },
      { provide: WORKBENCH_PORTS, useValue: ports },
      providePanelWindows(desktop.window('main'), { hostCommands: options.hostCommands ?? (() => false), ...(options.title ? { hostTitle: signal(options.title) } : {}) }),
    ],
  });
  return { desktop, ctx, shown, timers, windows: TestBed.inject(PanelWindows) };
}

describe('PanelWindows', () => {
  it('is the core PANEL_WINDOWS port', () => {
    const { windows } = setup();
    expect(TestBed.inject(PANEL_WINDOWS)).toBe(windows);
  });

  it('pops a panel out, tracks it and tells the dock when its window closes', async () => {
    const { windows, desktop } = setup();
    await TestBed.inject(ShellContextHost).load();
    const returned: string[] = [];
    const off = windows.onReturned((p) => returned.push(p));
    expect(await windows.popOut('wt.log', { x: 10, y: 20, width: 500, height: 300, scale: 2 })).toBe(true);
    expect(windows.poppedOut()).toEqual(['wt.log']);
    expect(desktop.all()[0]).toMatchObject({ panel: 'wt.log', title: 'Log', bounds: { x: 10, y: 20, width: 500, height: 300, scale: 2 } });
    // Popping out what is out already focuses it.
    expect(await windows.popOut('wt.log')).toBe(true);
    expect(desktop.window('main').focusLog).toEqual(['wt.log']);
    expect(windows.windows()).toHaveLength(1);
    // An instance of a view with instances is its own panel.
    expect(await windows.popOut('wt.code#src/main.rs')).toBe(true);
    expect(windows.poppedOut()).toEqual(['wt.log', 'wt.code#src/main.rs']);
    // The user closes the OS window: the dock hears it once.
    desktop.closeWindow('p-main-1');
    await settle();
    expect(returned).toEqual(['wt.log']);
    expect(windows.poppedOut()).toEqual(['wt.code#src/main.rs']);
    off();
    desktop.closeWindow('p-main-2');
    await settle();
    expect(returned).toEqual(['wt.log']);
    expect(windows.poppedOut()).toEqual([]);
  });

  it('refuses a view that is unknown, not offered or not an instance view, without opening anything', async () => {
    const { windows, desktop } = setup();
    await TestBed.inject(ShellContextHost).load();
    expect(await windows.popOut('wt.nothing')).toBe(false);
    expect(await windows.popOut('wt.hidden')).toBe(false);
    expect(await windows.popOut('wt.log#key')).toBe(false);
    expect(desktop.all()).toEqual([]);
  });

  it('keeps the panel docked and says why when the window cannot be opened', async () => {
    const { windows, desktop } = setup();
    await TestBed.inject(ShellContextHost).load();
    vi.spyOn(desktop.window('main'), 'popOut').mockRejectedValueOnce(new Error('the window system is busy'));
    expect(await windows.popOut('wt.log')).toBe(false);
    expect(shellPorts().messages()).toEqual(['Could not pop out Log: the window system is busy. It stays docked.']);
    expect(shellPorts().notices[0]?.level).toBe('error');
    expect(windows.poppedOut()).toEqual([]);
    vi.spyOn(desktop.window('main'), 'popOut').mockRejectedValueOnce('plain text');
    expect(await windows.popOut('wt.log')).toBe(false);
    vi.spyOn(desktop.window('main'), 'popOut').mockRejectedValueOnce({ message: 'from Rust' });
    expect(await windows.popOut('wt.log')).toBe(false);
    expect(shellPorts().messages().slice(1)).toEqual(['Could not pop out Log: plain text. It stays docked.', 'Could not pop out Log: from Rust. It stays docked.']);
  });

  it('allows at most twelve pop-outs per window', async () => {
    const { windows } = setup();
    await TestBed.inject(ShellContextHost).load();
    for (let i = 0; i < PANEL_LIMITS.maxPopOuts; i++) expect(await windows.popOut(`wt.code#${i}`)).toBe(true);
    expect(windows.canPopOut()).toBe(false);
    expect(await windows.popOut('wt.log')).toBe(false);
    expect(shellPorts().messages().at(-1)).toContain('At most 12 panels');
    expect(shellPorts().notices.at(-1)?.level).toBe('warning');
  });

  it('does not open the same panel twice at once', async () => {
    const { windows, desktop } = setup();
    await TestBed.inject(ShellContextHost).load();
    const [a, b] = await Promise.all([windows.popOut('wt.log'), windows.popOut('wt.log')]);
    expect([a, b].sort()).toEqual([false, true]);
    expect(desktop.all()).toHaveLength(1);
  });

  it('docks a panel back through its window, or by itself when the window is already gone', async () => {
    const { windows, desktop } = setup();
    await TestBed.inject(ShellContextHost).load();
    const returned: string[] = [];
    windows.onReturned((p) => returned.push(p));
    await windows.popOut('wt.log');
    await windows.dockBack('wt.log');
    await settle();
    expect(returned).toEqual(['wt.log']);
    await windows.dockBack('wt.log');
    expect(returned).toEqual(['wt.log']);
    await windows.popOut('wt.log');
    vi.spyOn(desktop.window('main'), 'dockBack').mockRejectedValueOnce(new Error('gone'));
    await windows.dockBack('wt.log');
    expect(returned).toEqual(['wt.log', 'wt.log']);
    expect(windows.poppedOut()).toEqual([]);
  });

  it('adopts the pop-outs the shell still has after the host window reloaded', async () => {
    const desktop = new InPageDesktop();
    await desktop.window('main').popOut({ panel: 'wt.log', title: 'Log' });
    const ctx = newCtx();
    TestBed.resetTestingModule();
    TestBed.configureTestingModule({ providers: [...windowProviders(ctx), { provide: SYNC_TIMERS, useValue: new FakeSyncTimers() }, providePanelWindows(desktop.window('main'))] });
    const windows = TestBed.inject(PanelWindows);
    await windows.start();
    expect(windows.poppedOut()).toEqual(['wt.log']);
    // A list the shell cannot give leaves what is known.
    vi.spyOn(desktop.window('main'), 'list').mockRejectedValueOnce(new Error('x'));
    desktop.window('main').emitPanels({ kind: 'changed' });
    await settle();
    expect(windows.poppedOut()).toEqual(['wt.log']);
  });

  it('reports a pop-out dragged back over the window, by the OS cursor or by its title bar', async () => {
    const { windows, desktop } = setup();
    await TestBed.inject(ShellContextHost).load();
    await windows.popOut('wt.log');
    const moved: PopOutMoved[] = [];
    const ended: string[] = [];
    const offMoved = windows.onMoved((e) => moved.push(e));
    const offEnd = windows.onDragEnd((l) => ended.push(l));
    desktop.dragWindow('p-main-1', 5, 6);
    expect(moved).toEqual([{ panel: 'wt.log', label: 'p-main-1', x: 5, y: 6, source: 'os' }]);
    // The title bar of a pop-out in a browser reports its own drag through the sync commands.
    const sync = TestBed.inject(UiSync);
    expect(sync.started).toBe(true);
    const popSync = TestBed.runInInjectionContext(() => new UiSync());
    popSync.start(desktop.window('p-main-1'), 'popout', 'p-main-1', []);
    popSync.runInHost('dragOver', JSON.stringify({ label: 'p-main-1', x: 7, y: 8 }));
    popSync.runInHost('dragOver', 'not json');
    popSync.runInHost('dragOver', JSON.stringify({ label: 'p-main-9', x: 1, y: 1 }));
    popSync.runInHost('dragEnd', 'p-main-1');
    await settle();
    expect(moved.at(-1)).toEqual({ panel: 'wt.log', label: 'p-main-1', x: 7, y: 8, source: 'handle' });
    expect(moved).toHaveLength(2);
    expect(ended).toEqual(['p-main-1']);
    offMoved();
    offEnd();
    desktop.dragWindow('p-main-1', 9, 9);
    expect(moved).toHaveLength(2);
    popSync.stop();
    expect(parseDragOver('{"label":"a","x":1,"y":2}')).toEqual({ label: 'a', x: 1, y: 2 });
    expect(parseDragOver('{"label":1}')).toBeNull();
  });

  it('runs the commands a pop-out forwards: show a view always, others only when allowed', async () => {
    const { windows, desktop, shown, ctx } = setup({ hostCommands: (id) => id === 'wt.undo' });
    await TestBed.inject(ShellContextHost).load();
    await windows.popOut('wt.log');
    const popSync = TestBed.runInInjectionContext(() => new UiSync());
    popSync.start(desktop.window('p-main-1'), 'popout', 'p-main-1', []);
    popSync.runInHost('showView', 'wt.code');
    popSync.runInHost('showView', 'wt.unknown');
    popSync.runInHost('run', 'wt.undo');
    popSync.runInHost('run', 'wt.format-disk');
    popSync.runInHost('whatever', 'x');
    await settle();
    expect(shown).toEqual(['wt.code']);
    expect(ctx.ran).toEqual(['wt.undo']);
    popSync.stop();
  });

  it('starts the ghost only where there is one, and only when another pop-out fits', async () => {
    const none = setup();
    await TestBed.inject(ShellContextHost).load();
    expect(none.windows.beginDragOut('wt.log', Promise.resolve(null))).toBeNull();
    const ghost = new MockGhost();
    const { windows } = setup({ ghost });
    await TestBed.inject(ShellContextHost).load();
    const request = { title: 'Log', width: 400, height: 300, grabX: 10, grabY: 5 };
    const preview = windows.beginDragOut('wt.log', Promise.resolve(request))!;
    expect(preview).not.toBeNull();
    preview.show(true);
    ghost.pointerAt(100, 100);
    const where = await preview.finish();
    expect(where).toMatchObject({ x: 90, y: 95, width: 400, height: 300, scale: 1 });
    // What was dropped is what pops out, exactly there.
    expect(await windows.popOut('wt.log', where!)).toBe(true);
    expect(windows.beginDragOut('wt.log', Promise.resolve(request))).toBeNull();
    expect(await windows.pointerDown()).toBe(false);
  });

  it('sends the host name along and takes shared state by the registry definitions', async () => {
    const { windows, desktop } = setup({ title: 'Incident 7' });
    await TestBed.inject(ShellContextHost).load();
    await windows.popOut('wt.log');
    const values = syncedValues(TestBed.inject(WorkbenchRegistry), 'host');
    expect(values.map((v) => v.key)).toEqual(['wt.query', 'wt.theme']);
    expect(HOST_TITLE_KEY).toBe('pd.panels.host-title');
    expect(desktop.all()).toHaveLength(1);
  });
});

describe('syncedValues', () => {
  it('reads the contexts, applies only what parses and gives dialogs the display scope', async () => {
    const { ctx } = setup();
    await TestBed.inject(ShellContextHost).load();
    const registry = TestBed.inject(WorkbenchRegistry);
    const [query, theme] = syncedValues(registry, 'popout');
    expect(query?.key).toBe('wt.query');
    ctx.query.set('a');
    expect(query?.read()).toBe('a');
    query?.apply('b');
    expect(ctx.query()).toBe('b');
    query?.apply(42);
    expect(ctx.query()).toBe('b');
    expect(theme?.scope).toBe('display');
    expect(syncedValues(registry, 'dialog').map((v) => v.key)).toEqual(['wt.theme']);
  });
});
