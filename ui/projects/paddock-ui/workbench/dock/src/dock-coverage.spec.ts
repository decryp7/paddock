import { ChangeDetectionStrategy, Component, ErrorHandler, inject, signal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { settle } from '@decryp7/paddock-ui/shell/testing';
import { InMemoryDockLayoutStore } from './dock-store';
import { MissingViewRenderer } from './angular-panel';
import { DockMenu } from './dock-menu';
import type { PanelWindowsPort, ViewDef } from '@decryp7/paddock-ui/workbench';
import { at, must } from './must.spec-helper';
import { mountDock, probeView, type HarnessContext } from './harness.spec-helper';

const VIEWS = [probeView('a.one'), probeView('a.two'), probeView('a.side', 'side'), probeView('a.side2', 'side')];

const key = (type: string, init: KeyboardEventInit) => document.dispatchEvent(new KeyboardEvent(type, { bubbles: true, cancelable: true, ...init }));

describe('dock keys', () => {
  it('leaves keys the switcher does not use alone while it is open', async () => {
    const { dock, rendered } = await mountDock({ views: VIEWS });
    await dock.showView('a.one');
    await dock.showView('a.two');
    await settle(rendered.fixture);
    key('keydown', { key: 'Tab', ctrlKey: true });
    const stray = new KeyboardEvent('keydown', { key: 'x', bubbles: true, cancelable: true });
    document.dispatchEvent(stray);
    expect(stray.defaultPrevented).toBe(false);
    expect(dock.switcher()).not.toBeNull();
    dock.cancelSwitch();
  });
});

describe('the switcher list', () => {
  it('follows the pointer and switches on a click', async () => {
    const { dock, rendered } = await mountDock({ views: VIEWS });
    await dock.showView('a.one');
    await dock.showView('a.two');
    await settle(rendered.fixture);
    key('keydown', { key: 'Tab', ctrlKey: true });
    await settle(rendered.fixture);
    const items = [...rendered.element.querySelectorAll<HTMLElement>('[role="option"]')];
    const target = items.find((i) => i.textContent?.includes('A.ONE'));
    expect(target).toBeDefined();
    target!.dispatchEvent(new Event('pointerenter'));
    await settle(rendered.fixture);
    expect(target!.getAttribute('aria-selected')).toBe('true');
    target!.click();
    await settle(rendered.fixture);
    expect(dock.switcher()).toBeNull();
    expect(dock.activePanel()).toBe('a.one');
  });
});

describe('auto-hide', () => {
  it('pins back only the given windows that are still auto-hidden, and closes the flyout with Escape on its tab', async () => {
    const { dock, rendered } = await mountDock({ views: VIEWS });
    await dock.showView('a.one');
    await dock.showView('a.side');
    await dock.showView('a.side2');
    await settle(rendered.fixture);
    dock.unpin('a.side', 'right');
    await settle(rendered.fixture);
    dock.pinIds(['a.side', 'a.side2', 'gone']);
    await settle(rendered.fixture);
    expect(dock.autoHide().items).toEqual([]);
    dock.unpin('a.side', 'right');
    await settle(rendered.fixture);
    const tab = rendered.one<HTMLButtonElement>('[data-ah-id="a.side"]');
    tab.dispatchEvent(new MouseEvent('mouseenter'));
    tab.dispatchEvent(new MouseEvent('mouseleave'));
    tab.dispatchEvent(new MouseEvent('mouseenter'));
    await settle(rendered.fixture);
    tab.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    await settle(rendered.fixture);
    expect(dock.autoHide().open).toBeNull();
  });

  it('offers the side choice from the group header button, by right click and by keyboard', async () => {
    const { dock, rendered } = await mountDock({ views: VIEWS });
    await dock.showView('a.one');
    await dock.showView('a.side');
    await settle(rendered.fixture);
    const button = [...rendered.element.querySelectorAll<HTMLButtonElement>('.pd-dock-autohide')].find((b) => !b.hidden)!;
    expect(button.getAttribute('aria-label')).toContain('A.SIDE');
    const menu = TestBed.inject(DockMenu);
    button.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: 5, clientY: 6 }));
    expect(menu.state()?.items.map((i) => i.label)).toEqual(['Auto-hide to the left', 'Auto-hide to the right', 'Auto-hide to the bottom']);
    menu.close();
    button.dispatchEvent(new KeyboardEvent('keydown', { key: 'ContextMenu', bubbles: true, cancelable: true }));
    expect(menu.state()).not.toBeNull();
    menu.close();
    button.dispatchEvent(new KeyboardEvent('keydown', { key: 'a', bubbles: true, cancelable: true }));
    expect(menu.state()).toBeNull();
    button.dispatchEvent(new KeyboardEvent('keydown', { key: 'F10', shiftKey: true, bubbles: true, cancelable: true }));
    const left = menu.state()!.items[0]!;
    left.run();
    await settle(rendered.fixture);
    expect(dock.autoHide().items.map((i) => [i.id, i.side])).toEqual([['a.side', 'left']]);
  });
});

describe('the group header', () => {
  it('auto-hides the tool window of the group with its pin button', async () => {
    const { dock, rendered } = await mountDock({ views: VIEWS });
    await dock.showView('a.one');
    await dock.showView('a.side');
    await settle(rendered.fixture);
    [...rendered.element.querySelectorAll<HTMLButtonElement>('.pd-dock-autohide')].find((b) => !b.hidden)!.click();
    await settle(rendered.fixture);
    expect(dock.autoHide().items.map((i) => i.id)).toEqual(['a.side']);
  });

  it('maximizes and restores the group, and opens the panel menu', async () => {
    const { dock, rendered } = await mountDock({ views: VIEWS });
    await dock.showView('a.one');
    await settle(rendered.fixture);
    const buttons = [...rendered.element.querySelectorAll<HTMLButtonElement>('.pd-dock-actions button')];
    const [menuButton, maximize] = [buttons.find((b) => b.getAttribute('aria-haspopup') === 'menu')!, buttons[buttons.length - 1]!];
    expect(maximize.getAttribute('aria-label')).toBe('Maximize panel');
    maximize.click();
    await settle(rendered.fixture);
    expect(maximize.getAttribute('aria-label')).toBe('Restore panel size');
    maximize.click();
    await settle(rendered.fixture);
    expect(maximize.getAttribute('aria-label')).toBe('Maximize panel');
    menuButton.click();
    await settle(rendered.fixture);
    expect(TestBed.inject(DockMenu).state()).not.toBeNull();
  });

  it('opens the panel menu from the menu button of a tab', async () => {
    const { dock, rendered } = await mountDock({ views: VIEWS });
    await dock.showView('a.one');
    await settle(rendered.fixture);
    rendered.one<HTMLElement>('.pd-dock-tab-menu').click();
    expect(TestBed.inject(DockMenu).state()).not.toBeNull();
  });
});

describe('panels: width flag, failures and the missing view', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  /** A view whose component runs `run` when it is created. */
  const failing = (id: string, run: () => never | void) => {
    @Component({ selector: 'pd-failing', changeDetection: ChangeDetectionStrategy.OnPush, template: '' })
    class Failing {
      constructor() {
        run();
      }
    }
    return probeView(id, 'document', { load: () => Promise.resolve(Failing) });
  };

  it('marks a panel narrower than the threshold with data-narrow and clears it when it grows', async () => {
    const observers: ((entries: { contentRect: { width: number } }[]) => void)[] = [];
    vi.stubGlobal(
      'ResizeObserver',
      class {
        constructor(cb: (entries: { contentRect: { width: number } }[]) => void) {
          observers.push(cb);
        }
        observe(): void {}
        unobserve(): void {}
        disconnect(): void {}
      },
    );
    const { dock, rendered } = await mountDock({ views: VIEWS });
    await dock.showView('a.one');
    await settle(rendered.fixture);
    const panel = rendered.one('.pd-panel[data-view-id="a.one"]');
    for (const fire of observers) fire([{ contentRect: { width: 120 } }]);
    expect(panel.hasAttribute('data-narrow')).toBe(true);
    for (const fire of observers) fire([{ contentRect: { width: 1200 } }]);
    expect(panel.hasAttribute('data-narrow')).toBe(false);
    for (const fire of observers) fire([]);
    expect(panel.hasAttribute('data-narrow')).toBe(false);
  });

  it('shows the error card for an error reported through the ErrorHandler of the panel, a thrown string and a thrown number', async () => {
    const reported = failing('a.reported', () => inject(ErrorHandler).handleError(new Error('handled inside')));
    vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const cases: [ViewDef<HarnessContext>, string][] = [
      [reported, 'handled inside'],
      [failing('a.string', () => { throw 'plain text'; }), 'plain text'],
      [failing('a.number', () => { throw 7; }), 'unknown error'],
    ];
    for (const [view, text] of cases) {
      const { dock, rendered } = await mountDock({ views: [view] });
      await dock.showView(view.id);
      await settle(rendered.fixture);
      await settle(rendered.fixture);
      expect(rendered.one('.pd-panel-failed strong').textContent).toContain(text);
    }
  });

  it('shows the original of a non-Error thrown while a panel renders, not the wrapper Angular puts around it', async () => {
    vi.spyOn(console, 'error').mockImplementation(() => undefined);
    @Component({ selector: 'pd-thrower', changeDetection: ChangeDetectionStrategy.OnPush, template: '{{ boom() }}' })
    class Thrower {
      boom(): string {
        throw 'template went wrong';
      }
    }
    const { dock, rendered } = await mountDock({ views: [probeView('a.render', 'document', { load: () => Promise.resolve(Thrower) })] });
    await dock.showView('a.render');
    await settle(rendered.fixture);
    await settle(rendered.fixture);
    expect(rendered.one('.pd-panel-failed strong').textContent).toContain('template went wrong');
  });

  it('copies the details of a failure, and says when the clipboard refuses', async () => {
    const writeText = vi.fn<(text: string) => Promise<void>>();
    vi.stubGlobal('navigator', { ...navigator, clipboard: { writeText } });
    vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const { dock, rendered } = await mountDock({ views: [failing('a.broken', () => { throw new Error('kaput'); })] });
    await dock.showView('a.broken');
    await settle(rendered.fixture);
    await settle(rendered.fixture);
    const copy = [...rendered.element.querySelectorAll<HTMLButtonElement>('.pd-panel-actions button')].find((b) => b.dataset['testid'] !== 'pd-panel-retry')!;
    writeText.mockResolvedValueOnce();
    copy.click();
    await settle(rendered.fixture);
    expect(writeText.mock.calls[0]?.[0]).toContain('Error: kaput');
    expect(copy.textContent).toBe('Copied');
    writeText.mockRejectedValueOnce(new Error('denied'));
    copy.click();
    await settle(rendered.fixture);
    expect(copy.textContent).toBe('Copy failed');
  });

  it('puts a placeholder where the view of a saved panel no longer exists', () => {
    const missing = new MissingViewRenderer('gone.view');
    expect(missing.element.textContent).toContain('gone.view');
    expect(missing.element.classList.contains('pd-panel-error')).toBe(true);
    missing.init();
  });
});

function windowsPort(result: boolean | 'throws' = true) {
  const popped = signal<readonly string[]>([]);
  const calls: [string, unknown][] = [];
  const port: PanelWindowsPort = {
    popOut: (panel, placement) => {
      calls.push([panel, placement]);
      if (result === 'throws') return Promise.reject(new Error('no window'));
      if (result) popped.update((p) => [...p, panel]);
      return Promise.resolve(result);
    },
    focus: vi.fn(),
    poppedOut: popped,
    onReturned: () => () => undefined,
  };
  return { port, calls };
}

describe('showing views that are away from the grid', () => {
  it('slides an auto-hidden window out when it is shown again, unless it is shown inactive', async () => {
    const { dock, rendered } = await mountDock({ views: VIEWS });
    await dock.showView('a.one');
    await dock.showView('a.side');
    await settle(rendered.fixture);
    dock.unpin('a.side', 'right');
    await settle(rendered.fixture);
    dock.show('a.side', { inactive: true });
    expect(dock.autoHide().open).toBeNull();
    dock.show('a.side');
    expect(dock.autoHide().open).toBe('a.side');
  });

  it('focuses the window of a popped-out view, unless it is shown inactive', async () => {
    const w = windowsPort();
    const { dock, rendered } = await mountDock({ views: VIEWS, windows: w.port });
    await dock.showView('a.one');
    await settle(rendered.fixture);
    expect(await dock.popOut('a.one')).toBe(true);
    dock.show('a.one', { inactive: true });
    expect(w.port.focus).not.toHaveBeenCalled();
    dock.show('a.one');
    expect(w.port.focus).toHaveBeenCalledWith('a.one');
  });
});

describe('panel content', () => {
  it('is the content element of the panel in front, and null for a panel behind another or one that is not open', async () => {
    const { dock, rendered } = await mountDock({ views: VIEWS });
    await dock.showView('a.one');
    await dock.showView('a.two');
    await settle(rendered.fixture);
    expect(dock.panelContent('a.two')).toBeInstanceOf(HTMLElement);
    expect(dock.panelContent('a.one')).toBeNull();
    expect(dock.panelContent('nope')).toBeNull();
  });
});

describe('pop-out', () => {
  it('keeps the panel docked and offers Retry when the window service throws', async () => {
    const w = windowsPort('throws');
    const { dock, rendered } = await mountDock({ views: VIEWS, windows: w.port });
    await dock.showView('a.one');
    await settle(rendered.fixture);
    expect(await dock.popOut('a.one')).toBe(false);
    expect(dock.notice()?.key).toBe('popout');
    expect(dock.openPanels()).toEqual(['a.one']);
  });

  it('pops a tab out where it is released outside the window, with the cue shown while it is dragged', async () => {
    const w = windowsPort();
    const { dock, rendered } = await mountDock({ views: VIEWS, windows: w.port });
    await dock.showView('a.one');
    await dock.showView('a.two');
    await settle(rendered.fixture);
    // jsdom has no hit testing: nothing is under the pointer, as outside the window.
    const hit = Object.assign(document, { elementsFromPoint: () => [] as Element[], elementFromPoint: () => null });
    expect(hit).toBe(document);
    const tab = rendered.one('[data-testid="pd-dock-tab"][data-panel-id="a.two"]');
    const pointer = (type: string, target: EventTarget, x: number, y: number) =>
      target.dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, button: 0, pointerId: 3, clientX: x, clientY: y, screenX: x + 50, screenY: y + 60 }));
    pointer('pointerdown', tab, 10, 10);
    pointer('pointermove', window, 400, 300);
    pointer('pointermove', window, -80, 300);
    pointer('pointerup', window, -80, 300);
    await settle(rendered.fixture);
    expect(w.calls).toHaveLength(1);
    expect(w.calls[0]?.[0]).toBe('a.two');
    expect(dock.openPanels()).toContain('a.two');
    expect(dock.isVisible('a.two')).toBe(true);
    delete (document as Partial<Document>).elementsFromPoint;
    delete (document as Partial<Document>).elementFromPoint;
  });

  it('pops the panel out of the panel menu action', async () => {
    const w = windowsPort();
    const { dock, rendered } = await mountDock({ views: VIEWS, windows: w.port });
    await dock.showView('a.one');
    await settle(rendered.fixture);
    dock.runPanelAction('a.one', 'popout');
    await settle(rendered.fixture);
    expect(w.calls.map((c) => c[0])).toEqual(['a.one']);
  });
});

describe('docking a tool as a document', () => {
  it('moves a tool that has no document group to beside into a group of its own', async () => {
    const { dock, rendered } = await mountDock({ views: VIEWS });
    await dock.showView('a.side');
    await dock.showView('a.side2');
    await settle(rendered.fixture);
    dock.runPanelAction('a.side2', 'dockAsDocument');
    await settle(rendered.fixture);
    expect(dock.openPanels()).toEqual(['a.side', 'a.side2']);
  });
});

describe('a saved layout that dockview cannot restore', () => {
  beforeEach(() => vi.spyOn(console, 'warn').mockImplementation(() => undefined));

  it('starts with the default layout and forgets the broken one', async () => {
    const store = new InMemoryDockLayoutStore();
    store.entries.set('wb.layout', JSON.stringify({ version: 1, layout: { grid: 'broken' }, extras: {} }));
    const { dock, rendered } = await mountDock({ views: VIEWS, store });
    expect(dock.ready()).toBe(true);
    await dock.showView('a.one');
    await settle(rendered.fixture);
    expect(dock.openPanels()).toEqual(['a.one']);
    expect(console.warn).toHaveBeenCalledWith(expect.stringContaining('could not be restored'), expect.any(String));
  });

  it('applySaved falls back to the default and says no', async () => {
    const { dock, rendered } = await mountDock({ views: VIEWS });
    await dock.showView('a.one');
    await settle(rendered.fixture);
    expect(dock.applySaved({ layout: { grid: 'broken' } as never, extras: { autoHidden: [], pinnedTabs: [] } })).toBe(false);
    expect(dock.ready()).toBe(true);
  });

  it('keeps the current arrangement when the layout an Undo goes back to cannot be restored', async () => {
    const { dock, rendered } = await mountDock({ views: VIEWS });
    await dock.showView('a.one');
    await settle(rendered.fixture);
    const inner = dock as unknown as { fitted: unknown; undoFit(): void };
    inner.fitted = { layout: { grid: 'broken' }, extras: { autoHidden: [], pinnedTabs: [] } };
    inner.undoFit();
    expect(console.warn).toHaveBeenCalledWith(expect.stringContaining('could not be restored'), expect.any(String));
    expect(dock.ready()).toBe(true);
  });
});

describe('the test accessors', () => {
  it('count from the end and fail the test with a clear message for a missing element', () => {
    expect(at([1, 2, 3], -1)).toBe(3);
    expect(() => at([1], 4)).toThrow('expected an element at index 4 (length 1)');
    expect(() => at([1], -2)).toThrow('expected an element at index -2');
    expect(must(0)).toBe(0);
    expect(() => must(null, 'the panel')).toThrow('expected the panel to be present');
  });
});
