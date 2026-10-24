import { NOTICE_ACTION_HANDLERS, type NoticeActionHandler } from '@decryp7/paddock-ui/shell/notify';
import { shellPorts } from '@decryp7/paddock-ui/shell/testing';
import type { ShellNotice } from '@decryp7/paddock-ui/shell';
import { DOCK_NOTICE_ACTION } from './dock-notice';
import { signal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { msg, CommandService } from '@decryp7/paddock-ui/shell';
import { settle, contributionProblems } from '@decryp7/paddock-ui/shell/testing';
import { Injector } from '@angular/core';
import { definePerspective, type PanelWindowsPort } from '@decryp7/paddock-ui/workbench';
import { DOCK_COMMANDS } from './dock-contributions';
import { DockContext } from './dock-context';
import { DockLayout } from './dock-layout';
import { DockMenu } from './dock-menu';
import { Perspectives } from './perspectives';
import { mountDock, probeView, tabIds } from './harness.spec-helper';

const perspective = (id: string, order: number, views: string[], focus?: string) =>
  definePerspective({ id, title: msg(`h.${id}`, id), icon: 'pd:x', order, views, ...(focus ? { focus } : {}) });

const VIEWS = [probeView('a.one'), probeView('a.two'), probeView('a.three'), probeView('a.side', 'side'), probeView('a.status', 'status')];

describe('perspectives', () => {
  const P1 = perspective('p.one', 1, ['a.one', 'a.side']);
  const P2 = perspective('p.two', 2, ['a.two', 'a.three'], 'a.three');

  it('starts in the first perspective and switches to another, closing what it does not list', async () => {
    const { dock, rendered } = await mountDock({ views: VIEWS, perspectives: [P1, P2] });
    expect(dock.activePerspective()).toBe('p.one');
    expect(dock.openPanels()).toEqual(['a.one', 'a.side']);
    expect(await dock.applyPerspective('p.two')).toBe(true);
    await settle(rendered.fixture);
    expect(dock.activePerspective()).toBe('p.two');
    expect(dock.openPanels()).toEqual(['a.three', 'a.two']);
    expect(dock.activePanel()).toBe('a.three');
    expect(await dock.applyPerspective('nope')).toBe(false);
    expect(await dock.applyPerspective('p.two')).toBe(true);
  });

  it('keeps each perspective as the user left it', async () => {
    const { dock, rendered } = await mountDock({ views: VIEWS, perspectives: [P1, P2] });
    await dock.showView('a.status', { background: true });
    await dock.applyPerspective('p.two');
    await settle(rendered.fixture);
    await dock.applyPerspective('p.one');
    await settle(rendered.fixture);
    expect(dock.openPanels()).toContain('a.status');
    expect(dock.openPanels()).toContain('a.one');
    const perspectives = TestBed.inject(Perspectives);
    perspectives.reset();
    await settle(rendered.fixture);
    expect(dock.openPanels()).toEqual(['a.one', 'a.side']);
  });

  it('switches with Ctrl+1..9 commands and offers a command only for a perspective that exists', async () => {
    const { dock, rendered } = await mountDock({ views: VIEWS, perspectives: [P1, P2] });
    const commands = TestBed.inject(CommandService);
    expect(commands.find(DOCK_COMMANDS.perspective(3))).toBeDefined();
    expect(commands.available().some((c) => c.id === DOCK_COMMANDS.perspective(3))).toBe(false);
    expect(await commands.run(DOCK_COMMANDS.perspective(2))).toBe(true);
    await settle(rendered.fixture);
    expect(dock.activePerspective()).toBe('p.two');
    expect(TestBed.inject(Perspectives).activeNumber()).toBe(2);
  });

  it('hides a perspective whose when does not hold', async () => {
    const hidden = definePerspective({ id: 'p.hid', title: msg('h.hid', 'Hid'), icon: 'pd:x', order: 3, views: ['a.one'], when: () => false });
    const { dock } = await mountDock({ views: VIEWS, perspectives: [P1, hidden] });
    expect(await dock.applyPerspective('p.hid')).toBe(false);
  });

  it('remembers the active perspective across windows', async () => {
    const first = await mountDock({ views: VIEWS, perspectives: [P1, P2] });
    await first.dock.applyPerspective('p.two');
    await settle(first.rendered.fixture);
    first.rendered.fixture.destroy();
    const second = await mountDock({ views: VIEWS, perspectives: [P1, P2], store: first.store });
    await TestBed.inject(Perspectives).ready;
    expect(second.dock.activePerspective()).toBe('p.two');
  });
});

describe('named layouts', () => {
  it('saves, applies and deletes a layout by name; a saved layout is a perspective too', async () => {
    const { dock, rendered } = await mountDock({ views: VIEWS });
    await dock.showView('a.one');
    await settle(rendered.fixture);
    expect(dock.saveNamedLayout('  Mine ')).toBe('Mine');
    expect(dock.saveNamedLayout('   ')).toBeNull();
    expect(dock.namedLayouts().map((l) => l.name)).toEqual(['Mine']);
    dock.closeView('a.one');
    await dock.showView('a.two');
    await settle(rendered.fixture);
    expect(dock.applyNamedLayout('Mine')).toBe(true);
    await settle(rendered.fixture);
    expect(dock.openPanels()).toEqual(['a.one']);
    expect(dock.applyNamedLayout('nope')).toBe(false);
    expect(TestBed.inject(Perspectives).list().map((p) => p.id)).toContain('saved:Mine');
    dock.deleteNamedLayout('Mine');
    expect(dock.namedLayouts()).toEqual([]);
  });

  it('runs the save, apply and delete commands', async () => {
    const { dock, rendered } = await mountDock({ views: VIEWS });
    const commands = TestBed.inject(CommandService);
    await dock.showView('a.one');
    expect(await commands.run(DOCK_COMMANDS.layoutSave, { name: 'X' })).toBe(true);
    expect(await commands.run(DOCK_COMMANDS.layoutSave)).toBe(true);
    expect(dock.namedLayouts().map((l) => l.name)).toEqual(['X', 'Layout 2']);
    expect(await commands.run(DOCK_COMMANDS.layoutApply, { name: 'X' })).toBe(true);
    expect(await commands.run(DOCK_COMMANDS.layoutDelete, { name: 'X' })).toBe(true);
    expect(dock.namedLayouts().map((l) => l.name)).toEqual(['Layout 2']);
    await settle(rendered.fixture);
  });
});

describe('auto-hide', () => {
  it('collapses a tool window to a side bar tab, slides it out and pins it back', async () => {
    const { dock, rendered } = await mountDock({ views: VIEWS });
    await dock.showView('a.one');
    await dock.showView('a.side');
    await settle(rendered.fixture);
    expect(dock.canAutoHide('a.side')).toBe(true);
    expect(dock.canAutoHide('a.one')).toBe(false);
    dock.unpin('a.side', 'right');
    await settle(rendered.fixture);
    expect(dock.autoHide().items.map((i) => i.id)).toEqual(['a.side']);
    expect(dock.openPanels()).toContain('a.side');
    const tab = rendered.one<HTMLButtonElement>('[data-ah-id="a.side"]');
    tab.click();
    await settle(rendered.fixture);
    expect(rendered.one('[data-testid="pd-dock-flyout"]').dataset['panelId']).toBe('a.side');
    expect(dock.isVisible('a.side')).toBe(true);
    dock.slideIn('a.side');
    await settle(rendered.fixture);
    expect(rendered.element.querySelector('[data-testid="pd-dock-flyout"]')).toBeNull();
    dock.pin('a.side');
    await settle(rendered.fixture);
    expect(dock.autoHide().items).toEqual([]);
    expect(tabIds(rendered.element)).toContain('a.side');
  });

  it('slides a status view out of the bottom edge instead of docking it', async () => {
    const { dock, rendered } = await mountDock({ views: VIEWS });
    await dock.showView('a.one');
    await dock.showView('a.status');
    await settle(rendered.fixture);
    expect(dock.autoHide().items.map((i) => [i.id, i.side])).toEqual([['a.status', 'bottom']]);
    expect(dock.autoHide().open).toBe('a.status');
    dock.closeView('a.status');
    expect(dock.autoHide().items).toEqual([]);
  });

  it('toggles auto-hide of the active tool window and hides or pins them all', async () => {
    const { dock, rendered } = await mountDock({ views: VIEWS });
    await dock.showView('a.one');
    await dock.showView('a.side');
    await settle(rendered.fixture);
    dock.toggleAutoHideActive();
    expect(dock.autoHide().items).toHaveLength(1);
    dock.autoHideEvent({ type: 'activate', id: 'a.side' });
    dock.toggleAutoHideActive();
    await settle(rendered.fixture);
    expect(dock.autoHide().items).toHaveLength(0);
    expect(dock.autoHideAll()).toEqual(['a.side']);
    dock.pinAll();
    expect(dock.autoHide().items).toEqual([]);
  });
});

describe('pop-out windows', () => {
  function windows() {
    const popped = signal<readonly string[]>([]);
    let returned: (panel: string) => void = () => undefined;
    const port: PanelWindowsPort & { calls: unknown[]; result: boolean } = {
      calls: [],
      result: true,
      popOut(panel, placement) {
        this.calls.push([panel, placement]);
        if (this.result) popped.update((p) => [...p, panel]);
        return Promise.resolve(this.result);
      },
      focus: vi.fn(),
      poppedOut: popped,
      onReturned: (l) => {
        returned = l;
        return () => undefined;
      },
    };
    return { port, popped, giveBack: (id: string) => { popped.update((p) => p.filter((x) => x !== id)); returned(id); } };
  }

  it('moves a panel out of the dock to its own window and back where it was', async () => {
    const w = windows();
    const { dock, rendered } = await mountDock({ views: VIEWS, windows: w.port });
    await dock.showView('a.one');
    await dock.showView('a.two');
    await settle(rendered.fixture);
    expect(dock.canPopOut('a.two')).toBe(true);
    expect(await dock.popOut('a.two', { at: { x: 10, y: 20 } })).toBe(true);
    await settle(rendered.fixture);
    expect(w.port.calls).toHaveLength(1);
    expect(tabIds(rendered.element)).toEqual(['a.one']);
    expect(dock.openPanels()).toEqual(['a.one', 'a.two']);
    expect(dock.isVisible('a.two')).toBe(true);
    await dock.showView('a.two');
    expect(w.port.focus).toHaveBeenCalledWith('a.two');
    w.giveBack('a.two');
    await settle(rendered.fixture);
    expect(tabIds(rendered.element).sort()).toEqual(['a.one', 'a.two']);
  });

  it('keeps the panel docked and says so, with Retry, when the window cannot open', async () => {
    const w = windows();
    w.port.result = false;
    const { dock, rendered } = await mountDock({ views: VIEWS, windows: w.port });
    await dock.showView('a.one');
    await settle(rendered.fixture);
    expect(await dock.popOut('a.one')).toBe(false);
    await settle(rendered.fixture);
    expect(tabIds(rendered.element)).toEqual(['a.one']);
    const posted = shellPorts().notices.at(-1)!;
    expect(posted).toMatchObject({ level: 'warning', kind: 'pd.wb.dock.popout', action: { type: DOCK_NOTICE_ACTION, key: 'popout', label: 'Retry' } });
    expect(rendered.element.querySelector('.pd-dock-notice')).toBeNull();
    w.port.result = true;
    await runNoticeButton(posted);
    await settle(rendered.fixture);
    expect(w.port.calls).toHaveLength(2);
  });

  it('hides the pop-out action without a pop-out service, and for a view that cannot leave', async () => {
    const without = await mountDock({ views: VIEWS });
    await without.dock.showView('a.one');
    expect(without.dock.canPopOut('a.one')).toBe(false);
    expect(await without.dock.popOut('a.one')).toBe(false);
    without.dock.openPanelMenu('a.one', null, { x: 1, y: 1 });
    expect(TestBed.inject(DockMenu).state()?.items.map((i) => i.label)).not.toContain('Pop out to window');
    const w = windows();
    const fixed = await mountDock({ views: [probeView('a.fixed', 'document', { popOut: false })], windows: w.port });
    await fixed.dock.showView('a.fixed');
    expect(fixed.dock.canPopOut('a.fixed')).toBe(false);
    expect(await fixed.dock.popOutActive()).toBe(false);
  });
});

describe('panel menu, switcher and keys', () => {
  it('opens the panel menu with the actions of the panel, and runs one', async () => {
    const { dock, rendered } = await mountDock({ views: VIEWS });
    await dock.showView('a.one');
    await dock.showView('a.two');
    await settle(rendered.fixture);
    dock.openPanelMenu('a.two', null, { x: 5, y: 5 });
    await settle(rendered.fixture);
    const items = rendered.all<HTMLButtonElement>('[data-testid="pd-dock-menu"] button');
    expect(items.map((b) => b.textContent)).toContain('Close');
    items.find((b) => b.textContent === 'Close')?.click();
    await settle(rendered.fixture);
    expect(dock.openPanels()).toEqual(['a.one']);
    expect(rendered.element.querySelector('[data-testid="pd-dock-menu"]')).toBeNull();
  });

  it('moves through the menu with the arrow keys and closes it with Escape', async () => {
    const { dock, rendered } = await mountDock({ views: VIEWS });
    await dock.showView('a.one');
    await settle(rendered.fixture);
    dock.openPanelMenu('a.one', null, { x: 5, y: 5 });
    await settle(rendered.fixture);
    const menu = rendered.one('[data-testid="pd-dock-menu"]');
    menu.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true }));
    menu.dispatchEvent(new KeyboardEvent('keydown', { key: 'End', bubbles: true }));
    menu.dispatchEvent(new KeyboardEvent('keydown', { key: 'Home', bubbles: true }));
    menu.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowUp', bubbles: true }));
    menu.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    await settle(rendered.fixture);
    expect(rendered.element.querySelector('[data-testid="pd-dock-menu"]')).toBeNull();
  });

  it('opens the switcher with Ctrl+Tab, cycles, and switches when Ctrl is released', async () => {
    const { dock, rendered } = await mountDock({ views: VIEWS });
    await dock.showView('a.one');
    await dock.showView('a.two');
    await settle(rendered.fixture);
    const key = (type: string, init: KeyboardEventInit) => document.dispatchEvent(new KeyboardEvent(type, { bubbles: true, cancelable: true, ...init }));
    key('keydown', { key: 'Tab', ctrlKey: true });
    await settle(rendered.fixture);
    expect(dock.switcher()).not.toBeNull();
    expect(rendered.element.querySelector('pd-dock-switcher')).not.toBeNull();
    key('keydown', { key: 'ArrowDown' });
    key('keydown', { key: 'ArrowUp' });
    key('keyup', { key: 'Control' });
    await settle(rendered.fixture);
    expect(dock.switcher()).toBeNull();
    expect(dock.activePanel()).toBe('a.one');
    key('keydown', { key: 'Tab', ctrlKey: true, shiftKey: true });
    key('keydown', { key: 'Escape' });
    expect(dock.switcher()).toBeNull();
    key('keydown', { key: 'Tab', ctrlKey: true });
    key('keydown', { key: 'Enter' });
    expect(dock.switcher()).toBeNull();
    key('keydown', { key: 'x' });
  });

  it('closes a focused tab with Delete unless its view cannot be closed', async () => {
    const { dock, rendered } = await mountDock({ views: [probeView('a.one'), probeView('a.pin', 'document', { closable: false })] });
    await dock.showView('a.one');
    await dock.showView('a.pin');
    await settle(rendered.fixture);
    const tab = (id: string) => rendered.one(`[data-testid="pd-dock-tab"][data-panel-id="${id}"]`);
    const before = [...dock.openPanels()];
    tab('a.pin').dispatchEvent(new KeyboardEvent('keydown', { key: 'Delete', bubbles: true }));
    expect([before, dock.openPanels()]).toEqual([['a.one', 'a.pin'], ['a.one', 'a.pin']]);
    tab('a.one').dispatchEvent(new KeyboardEvent('keydown', { key: 'Delete', bubbles: true }));
    expect(dock.openPanels()).toEqual(['a.pin']);
  });

  it('toggles full screen and the Window commands that act on the active panel', async () => {
    const { dock, rendered } = await mountDock({ views: VIEWS });
    await dock.showView('a.one');
    await dock.showView('a.two');
    await settle(rendered.fixture);
    dock.toggleFullScreen();
    expect(dock.fullScreen()).toBe(true);
    expect(document.documentElement.classList.contains('pd-dock-full-screen')).toBe(true);
    dock.toggleFullScreen();
    expect(dock.fullScreen()).toBe(false);
    const commands = TestBed.inject(CommandService);
    expect(await commands.run(DOCK_COMMANDS.panelMenu)).toBe(true);
    TestBed.inject(DockMenu).close();
    expect(await commands.run(DOCK_COMMANDS.float)).toBe(true);
    expect(await commands.run(DOCK_COMMANDS.closeAllDocuments)).toBe(true);
    expect(await commands.run(DOCK_COMMANDS.fullScreen)).toBe(true);
    expect(await commands.run(DOCK_COMMANDS.switcher)).toBe(true);
    dock.cancelSwitch();
    expect(await commands.run(DOCK_COMMANDS.autoHideAll)).toBe(true);
    expect(await commands.run(DOCK_COMMANDS.autoHide)).toBe(true);
    expect(await commands.run(DOCK_COMMANDS.layoutReset)).toBe(true);
    expect(await commands.run(DOCK_COMMANDS.pinAll)).toBe(false);
    expect(await commands.run(DOCK_COMMANDS.popOut)).toBe(false);
  });
});

describe('the layout fitted to a window', () => {
  it('says so once, with Undo, and withdraws the notice when everything is back', async () => {
    const { dock, rendered } = await mountDock({ views: VIEWS });
    const layout = TestBed.inject(DockLayout);
    const undo = vi.fn();
    layout.notifyFitted(undo);
    await settle(rendered.fixture);
    const posted = shellPorts().notices.at(-1)!;
    expect(posted).toMatchObject({ level: 'info', message: 'Layout adjusted to fit this window', kind: 'pd.wb.dock.layout-fit', action: { label: 'Undo' } });
    await runNoticeButton(posted);
    await settle(rendered.fixture);
    expect(undo).toHaveBeenCalledTimes(1);
    // The button of a notice that is gone does nothing.
    await runNoticeButton(posted);
    expect(undo).toHaveBeenCalledTimes(1);
    expect(dock.notice()).toBeNull();
    layout.notifyFitted(undo);
    layout.withdrawFitted();
    expect(dock.notice()).toBeNull();
    TestBed.inject(DockContext).notice.set({ key: 'other', message: 'm' });
    layout.withdrawFitted();
    expect(dock.notice()?.key).toBe('other');
    await settle(rendered.fixture);
    expect(shellPorts().notices.at(-1)).toMatchObject({ kind: 'pd.wb.dock.other', message: 'm' });
    expect(shellPorts().notices.at(-1)?.action).toBeUndefined();
  });
});

describe('contributions', () => {
  it('pass the contributions lint', async () => {
    await mountDock({ views: VIEWS });
    expect(contributionProblems(TestBed.inject(Injector))).toEqual([]);
  });
});

/** Presses the button of a posted notice as the notification host would (through the window's handlers). */
async function runNoticeButton(notice: ShellNotice): Promise<void> {
  const handlers = TestBed.inject(NOTICE_ACTION_HANDLERS) as unknown as NoticeActionHandler[];
  const handler = handlers.find((h) => h.type === notice.action?.type)!;
  expect(handler.label(notice.action as never, {} as never)).toBe(notice.action?.['label']);
  await handler.run(notice.action as never, {} as never);
}
