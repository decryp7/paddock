import { Injector, signal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { CommandService } from '@decryp7/paddock-ui/shell';
import { RecordingShellPorts, settle } from '@decryp7/paddock-ui/shell/testing';
import type { PanelWindowsPort } from '@decryp7/paddock-ui/workbench';
import { DOCK_COMMANDS } from './dock-contributions';
import { DockMenu } from './dock-menu';
import { DOCK_STYLES } from './dock-styles';
import { type DockLayoutStore, InMemoryDockLayoutStore } from './dock-store';
import { mountDock, probeView, tabIds } from './harness.spec-helper';
import { LAYOUT_MANAGE_DIALOG_ID, LAYOUT_SAVE_DIALOG_ID } from './layout-dialogs';
import { toEnvelope } from './layout-store';
import { Perspectives } from './perspectives';
import { WindowLayouts } from './window-layouts';

const VIEWS = [probeView('a.one'), probeView('a.two'), probeView('a.side', 'side')];

/** A pop-out service whose answer per panel is set by hand; records what was opened and with which placement. */
function windows(answer: (panel: string) => boolean | 'throws' = () => true) {
  const popped = signal<readonly string[]>([]);
  const opened: [string, unknown][] = [];
  const returned = new Set<(panel: string) => void>();
  const port: PanelWindowsPort = {
    popOut: (panel, placement) => {
      opened.push([panel, placement]);
      const a = answer(panel);
      if (a === 'throws') return Promise.reject(new Error('no window'));
      if (a) popped.update((p) => [...p, panel]);
      return Promise.resolve(a);
    },
    focus: vi.fn(),
    poppedOut: popped,
    onReturned: (l) => (returned.add(l), () => returned.delete(l)),
  };
  return { port, opened, popped, close: (panel: string) => returned.forEach((l) => l(panel)) };
}

describe('the dock before it has a layout (P2-K)', () => {
  it('draws a dock skeleton and no data-ready until the layout is restored, then removes it', async () => {
    let release!: (v: unknown) => void;
    const slow: DockLayoutStore = { load: () => new Promise((r) => (release = r)), save: () => Promise.resolve() };
    const { rendered, dock } = await mountDock({ views: VIEWS, store: slow });
    const host = rendered.one('[data-testid="pd-dock"]');
    expect(dock.ready()).toBe(false);
    expect(host.hasAttribute('data-ready')).toBe(false);
    expect(host.getAttribute('aria-busy')).toBe('true');
    expect(host.querySelector('[data-testid="pd-dock-skeleton"] .pd-dock-boot-strip')).not.toBeNull();
    release(null);
    await settle(rendered.fixture);
    await settle(rendered.fixture);
    expect(dock.ready()).toBe(true);
    expect(host.hasAttribute('data-ready')).toBe(true);
    expect(host.hasAttribute('aria-busy')).toBe(false);
    expect(host.querySelector('[data-testid="pd-dock-skeleton"]')).toBeNull();
  });

  it('keeps the tab testid and panel id on the label and gives the focus ring to the element that carries role=tab', async () => {
    const { rendered, dock } = await mountDock({ views: VIEWS });
    await dock.showView('a.one');
    await settle(rendered.fixture);
    const label = rendered.one<HTMLElement>('[data-testid="pd-dock-tab"][data-panel-id="a.one"]');
    const carrier = label.closest<HTMLElement>('[role="tab"]')!;
    expect(carrier).not.toBe(label);
    label.focus();
    expect(document.activeElement).toBe(carrier);
    expect(DOCK_STYLES).toMatch(/\.dv-tab:focus-visible \{ outline: 2px solid var\(--pd-focus-ring\)/);
  });
});

describe('the slide-out of an auto-hidden tool window (P3-7)', () => {
  it('has no header row, is as tall as its content (at most half the dock), starts under the groups\' tab strips and has the 12 px inset', async () => {
    const { rendered, dock } = await mountDock({ views: VIEWS });
    await dock.showView('a.one');
    await dock.showView('a.side');
    await settle(rendered.fixture);
    dock.unpin('a.side', 'right');
    await settle(rendered.fixture);
    rendered.one<HTMLButtonElement>('[data-ah-id="a.side"]').click();
    await settle(rendered.fixture);
    const flyout = rendered.one('[data-testid="pd-dock-flyout"]');
    expect(flyout.style.getPropertyValue('--pd-ah-size')).toMatch(/^\d+px$/);
    expect(flyout.querySelector('.pd-dock-ah-spacer')).toBeNull();
    expect(flyout.querySelectorAll('.pd-dock-ah-head button').length).toBe(2);
    expect(DOCK_STYLES).toMatch(/\.pd-dock-ah-flyout \{[^}]*height: auto; max-height: 50%;/s);
    expect(DOCK_STYLES).toMatch(/\.pd-dock-ah-flyout-right \{[^}]*top: var\(--pd-dock-strip-height, 34px\)/);
    expect(DOCK_STYLES).toMatch(/\.pd-dock-ah-flyout-bottom \{[^}]*max-height: min\(var\(--pd-ah-size\), 50%\)/);
    expect(DOCK_STYLES).toMatch(/\.pd-dock-ah-head \{[^}]*position: absolute/);
    expect(DOCK_STYLES).toMatch(/\.pd-dock-ah-body \{[^}]*padding: var\(--pd-space-3\)/);
  });
});

describe('the menu of the dock is never painted at the origin (P3-16)', () => {
  it('is hidden at the moment it is inserted into the page, and shown only once it is placed inside the window', async () => {
    const { rendered } = await mountDock({ views: VIEWS });
    const first: { visibility: string; placed: boolean; focused: boolean }[] = [];
    const record = (node: Node) => {
      const el = node instanceof HTMLElement ? (node.matches('.pd-dock-menu') ? node : node.querySelector<HTMLElement>('.pd-dock-menu')) : null;
      if (el) first.push({ visibility: getComputedStyle(el).visibility, placed: 'placed' in el.dataset, focused: el.contains(document.activeElement) });
    };
    const insertBefore = Node.prototype.insertBefore;
    const appendChild = Node.prototype.appendChild;
    vi.spyOn(Node.prototype, 'insertBefore').mockImplementation(function <T extends Node>(this: Node, node: T, child: Node | null): T {
      const r = insertBefore.call(this, node, child) as T;
      record(node);
      return r;
    });
    vi.spyOn(Node.prototype, 'appendChild').mockImplementation(function <T extends Node>(this: Node, node: T): T {
      const r = appendChild.call(this, node) as T;
      record(node);
      return r;
    });
    TestBed.inject(DockMenu).open(120, 80, [{ label: 'One', run: () => undefined }]);
    await settle(rendered.fixture);
    vi.restoreAllMocks();
    expect(first[0]).toEqual({ visibility: 'hidden', placed: false, focused: false });
    const menu = rendered.one('.pd-dock-menu');
    expect('placed' in menu.dataset).toBe(true);
    expect(getComputedStyle(menu).visibility).not.toBe('hidden');
    expect([menu.style.left, menu.style.top]).toEqual(['120px', '80px']);
    expect(document.activeElement?.closest('.pd-dock-menu')).toBe(menu);
  });
});

describe('panels popped out are saved with the layout (gap 1)', () => {
  const saveFrom = async (first: Awaited<ReturnType<typeof mountDock>>) => {
    const layout = first.dock.currentLayout()!;
    const store = new InMemoryDockLayoutStore();
    await store.save('wb.layout', toEnvelope(layout) as unknown as Record<string, unknown>);
    return { store, layout };
  };

  it('records a popped-out panel with its slot, and forgets it when its window docks back or it is closed', async () => {
    const w = windows();
    const first = await mountDock({ views: VIEWS, windows: w.port });
    await first.dock.showView('a.one');
    await first.dock.showView('a.two');
    await settle(first.rendered.fixture);
    expect(await first.dock.popOut('a.two')).toBe(true);
    expect(first.dock.currentLayout()?.extras.poppedOut?.map((p) => p.id)).toEqual(['a.two']);
    w.close('a.two');
    await settle(first.rendered.fixture);
    expect(first.dock.currentLayout()?.extras.poppedOut).toBeUndefined();
    expect(await first.dock.popOut('a.two')).toBe(true);
    first.dock.close('a.two');
    expect(first.dock.currentLayout()?.extras.poppedOut).toBeUndefined();
  });

  it('opens the panel in its window again on restore, at the place the engine remembers for it (no bounds)', async () => {
    const first = await mountDock({ views: VIEWS, windows: windows().port });
    await first.dock.showView('a.one');
    await first.dock.showView('a.two');
    await settle(first.rendered.fixture);
    await first.dock.popOut('a.two', { rect: { x: 5, y: 6, width: 300, height: 200 } });
    const { store } = await saveFrom(first);
    const w = windows();
    const second = await mountDock({ views: VIEWS, windows: w.port, store });
    expect(w.opened).toEqual([['a.two', undefined]]);
    expect(tabIds(second.rendered.element)).toEqual(['a.one']);
    expect(second.dock.openPanels()).toContain('a.two');
    expect(second.dock.currentLayout()?.extras.poppedOut?.map((p) => p.id)).toEqual(['a.two']);
    // Its window closing later docks it back where it was.
    w.close('a.two');
    await settle(second.rendered.fixture);
    expect(tabIds(second.rendered.element).sort()).toEqual(['a.one', 'a.two']);
  });

  it('docks the panel where it was when the window cannot open (refused or failing), with nothing left behind', async () => {
    for (const answer of [false, 'throws'] as const) {
      const first = await mountDock({ views: VIEWS, windows: windows().port });
      await first.dock.showView('a.one');
      await first.dock.showView('a.two');
      await settle(first.rendered.fixture);
      await first.dock.popOut('a.two');
      const { store } = await saveFrom(first);
      const second = await mountDock({ views: VIEWS, windows: windows(() => answer).port, store });
      await settle(second.rendered.fixture);
      expect(tabIds(second.rendered.element).sort()).toEqual(['a.one', 'a.two']);
      expect(second.dock.currentLayout()?.extras.poppedOut).toBeUndefined();
    }
  });

  it('docks it at once when this window has no pop-out service, and ignores a view this version no longer has', async () => {
    const first = await mountDock({ views: VIEWS, windows: windows().port });
    await first.dock.showView('a.one');
    await first.dock.showView('a.two');
    await settle(first.rendered.fixture);
    await first.dock.popOut('a.two');
    const { layout } = await saveFrom(first);
    const gone = { ...layout, extras: { ...layout.extras, poppedOut: [...(layout.extras.poppedOut ?? []), { id: 'a.removed', slot: { referencePanel: null, direction: 'within' as const } }] } };
    const store = new InMemoryDockLayoutStore();
    await store.save('wb.layout', toEnvelope(gone) as unknown as Record<string, unknown>);
    const second = await mountDock({ views: VIEWS, windows: null, store });
    expect(tabIds(second.rendered.element).sort()).toEqual(['a.one', 'a.two']);
  });

  it('does not dock a panel whose window failed after the dock was detached', async () => {
    const first = await mountDock({ views: VIEWS, windows: windows().port });
    await first.dock.showView('a.one');
    await first.dock.showView('a.two');
    await settle(first.rendered.fixture);
    await first.dock.popOut('a.two');
    const { store } = await saveFrom(first);
    let fail!: (v: boolean) => void;
    const late: PanelWindowsPort = { ...windows().port, popOut: () => new Promise<boolean>((r) => (fail = r)) };
    const second = await mountDock({ views: VIEWS, windows: late, store });
    second.rendered.fixture.destroy();
    fail(false);
    await Promise.resolve();
    expect(second.dock.currentLayout()).toBeNull();
  });
});

describe('save and manage layouts as dialogs (gap 2)', () => {
  const setup = async () => {
    const m = await mountDock({ views: VIEWS });
    await m.dock.showView('a.one');
    await settle(m.rendered.fixture);
    const ports = TestBed.inject(RecordingShellPorts);
    return { ...m, ports, commands: TestBed.inject(CommandService) };
  };

  it('is registered with commands, window-menu entries and both dialogs', async () => {
    const { commands } = await setup();
    for (const id of [DOCK_COMMANDS.layoutSave, DOCK_COMMANDS.layoutManage]) expect(await commands.run(id, { name: 'ignored-for-manage' }).catch(() => false)).toBeDefined();
    expect(DOCK_COMMANDS.layoutSave).toBe('pd.wb.dock.layout.save');
    expect(DOCK_COMMANDS.layoutManage).toBe('pd.wb.dock.layout.manage');
    expect(TestBed.inject(Injector)).toBeTruthy();
  });

  it('asks the Save layout dialog for a name (the names in use, a suggestion) and saves what it answers', async () => {
    const { dock, ports, commands } = await setup();
    dock.saveNamedLayout('Work');
    ports.dialogAnswers.set(LAYOUT_SAVE_DIALOG_ID, '  Debug ');
    expect(await commands.run(DOCK_COMMANDS.layoutSave)).toBe(true);
    expect(ports.dialogs.at(-1)).toEqual({ id: LAYOUT_SAVE_DIALOG_ID, inputs: { names: ['Work'], suggested: 'Layout 2' } });
    expect(dock.namedLayouts().map((l) => l.name)).toEqual(['Work', 'Debug']);
    expect(TestBed.inject(Perspectives).active()?.id).toBe('saved:Debug');
  });

  it('saves nothing when the dialog is cancelled, and saves at once when a name is given', async () => {
    const { dock, ports, commands } = await setup();
    expect(await commands.run(DOCK_COMMANDS.layoutSave)).toBe(true);
    expect(dock.namedLayouts()).toEqual([]);
    ports.dialogs.length = 0;
    await commands.run(DOCK_COMMANDS.layoutSave, { name: 'Direct' });
    expect(ports.dialogs).toEqual([]);
    expect(dock.namedLayouts().map((l) => l.name)).toEqual(['Direct']);
  });

  it('opens Manage layouts, reads the layouts the window edited, and applies the one it answers', async () => {
    const { dock, rendered, ports, store, commands } = await setup();
    dock.saveNamedLayout('Mine');
    await dock.showView('a.two');
    await settle(rendered.fixture);
    // The manage window renames "Mine" and deletes nothing, writing the store from its own window.
    const document = (await store.load('wb.layouts')) as { layouts: { name: string }[] };
    await store.save('wb.layouts', { ...document, layouts: document.layouts.map((l) => ({ ...l, name: 'Renamed' })) });
    ports.dialogAnswers.set(LAYOUT_MANAGE_DIALOG_ID, { apply: 'Renamed' });
    expect(await commands.run(DOCK_COMMANDS.layoutManage)).toBe(true);
    expect(ports.dialogs.at(-1)).toEqual({ id: LAYOUT_MANAGE_DIALOG_ID, inputs: {} });
    expect(dock.namedLayouts().map((l) => l.name)).toEqual(['Renamed']);
    await settle(rendered.fixture);
    expect(dock.openPanels()).toEqual(['a.one']);
  });

  it('forgets what was remembered for layouts the manage window deleted, and falls back to the first perspective', async () => {
    const { dock, rendered, ports, store, commands } = await setup();
    const perspectives = TestBed.inject(Perspectives);
    perspectives.saveCurrent('Gone');
    await settle(rendered.fixture);
    expect(perspectives.active()?.id).toBe('saved:Gone');
    await store.save('wb.layouts', { version: 1, layouts: [] });
    ports.dialogAnswers.set(LAYOUT_MANAGE_DIALOG_ID, undefined);
    await commands.run(DOCK_COMMANDS.layoutManage);
    expect(dock.namedLayouts()).toEqual([]);
    expect(perspectives.active()?.id ?? null).not.toBe('saved:Gone');
    // A layout that still exists keeps its place.
    expect(TestBed.inject(WindowLayouts).list()).toEqual([]);
  });
});
