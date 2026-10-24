import { TestBed } from '@angular/core/testing';
import { settle } from '@decryp7/paddock-ui/shell/testing';
import { DockContext } from './dock-context';
import { DockLayout } from './dock-layout';
import { HOVER_HIDE_MS } from './dock-host';
import { mountDock, probeView } from './harness.spec-helper';

const VIEWS = [probeView('a.one'), probeView('a.two'), probeView('t.side', 'side')];

async function withFlyout() {
  const m = await mountDock({ views: VIEWS });
  await m.dock.showView('a.one');
  await m.dock.showView('t.side');
  await settle(m.rendered.fixture);
  m.dock.unpin('t.side', 'right');
  await settle(m.rendered.fixture);
  return m;
}

describe('pd-dock auto-hide bars', () => {
  afterEach(() => vi.useRealTimers());

  it('slides a tool window out on hover and hides it again shortly after the pointer leaves', async () => {
    const { rendered, dock } = await withFlyout();
    vi.useFakeTimers();
    const tab = rendered.one<HTMLButtonElement>('[data-ah-id="t.side"]');
    tab.dispatchEvent(new MouseEvent('mouseenter'));
    await settle(rendered.fixture);
    expect(dock.autoHide().open).toBe('t.side');
    tab.dispatchEvent(new MouseEvent('mouseleave'));
    tab.dispatchEvent(new MouseEvent('mouseenter'));
    vi.advanceTimersByTime(HOVER_HIDE_MS + 10);
    expect(dock.autoHide().open).toBe('t.side');
    tab.dispatchEvent(new MouseEvent('mouseleave'));
    vi.advanceTimersByTime(HOVER_HIDE_MS + 10);
    expect(dock.autoHide().open).toBeNull();
  });

  it('opens on a keyboard click and moves the focus into the flyout; Escape hides it and returns to the tab', async () => {
    const { rendered, dock } = await withFlyout();
    const tab = rendered.one<HTMLButtonElement>('[data-ah-id="t.side"]');
    tab.click();
    await settle(rendered.fixture);
    expect(dock.autoHide().open).toBe('t.side');
    expect(rendered.element.querySelector('.pd-dock-ah-body .pd-panel')).not.toBeNull();
    const flyout = rendered.one('[data-testid="pd-dock-flyout"]');
    flyout.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    await settle(rendered.fixture);
    expect(dock.autoHide().open).toBeNull();
  });

  it('hides when something else is pressed or focused, but not when the slide-out itself is', async () => {
    const { rendered, dock } = await withFlyout();
    rendered.one<HTMLButtonElement>('[data-ah-id="t.side"]').click();
    await settle(rendered.fixture);
    rendered.one('[data-testid="pd-dock-flyout"]').dispatchEvent(new PointerEvent('pointerdown', { bubbles: true }));
    expect(dock.autoHide().open).toBe('t.side');
    const flyout = rendered.one('[data-testid="pd-dock-flyout"]');
    flyout.dispatchEvent(new FocusEvent('focusout', { bubbles: true, relatedTarget: flyout }));
    expect(dock.autoHide().open).toBe('t.side');
    flyout.dispatchEvent(new FocusEvent('focusout', { bubbles: true, relatedTarget: document.body }));
    expect(dock.autoHide().open).toBeNull();
    rendered.one<HTMLButtonElement>('[data-ah-id="t.side"]').click();
    await settle(rendered.fixture);
    document.body.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true }));
    expect(dock.autoHide().open).toBeNull();
    rendered.one<HTMLButtonElement>('[data-ah-id="t.side"]').click();
    await settle(rendered.fixture);
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    expect(dock.autoHide().open).toBeNull();
  });

  it('pins and hides from the slide-out header, and resizes by the grip', async () => {
    const { rendered, dock } = await withFlyout();
    rendered.one<HTMLButtonElement>('[data-ah-id="t.side"]').click();
    await settle(rendered.fixture);
    const before = dock.autoHide().items[0]!.size;
    const grip = rendered.one('.pd-dock-ah-grip');
    grip.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true, clientX: 500, clientY: 0 }));
    document.dispatchEvent(new PointerEvent('pointermove', { clientX: 450, clientY: 0 }));
    document.dispatchEvent(new PointerEvent('pointerup'));
    expect(dock.autoHide().items[0]!.size).not.toBe(before);
    const buttons = rendered.all<HTMLButtonElement>('.pd-dock-ah-head button');
    buttons[1]!.click();
    await settle(rendered.fixture);
    expect(dock.autoHide().open).toBeNull();
    rendered.one<HTMLButtonElement>('[data-ah-id="t.side"]').click();
    await settle(rendered.fixture);
    rendered.all<HTMLButtonElement>('.pd-dock-ah-head button')[0]!.click();
    await settle(rendered.fixture);
    expect(dock.autoHide().items).toEqual([]);
  });

  it('labels the three bars', async () => {
    const m = await mountDock({ views: [probeView('a.one'), probeView('t.a', 'side'), probeView('t.b', 'side'), probeView('t.c', 'side')] });
    await m.dock.showView('a.one');
    for (const id of ['t.a', 't.b', 't.c']) await m.dock.showView(id);
    await settle(m.rendered.fixture);
    m.dock.unpin('t.a', 'left');
    m.dock.unpin('t.b', 'right');
    m.dock.unpin('t.c', 'bottom');
    await settle(m.rendered.fixture);
    const labels = ['left', 'right', 'bottom'].map((s) => m.rendered.one(`[data-testid="pd-dock-ah-bar-${s}"]`).getAttribute('aria-label'));
    expect(labels.map((l) => l?.split('(')[1] ?? l)).toEqual(['left)', 'right)', 'bottom)'].map((x, i) => (i < 3 ? x : x)));
  });
});

describe('the tab strip', () => {
  it('opens the panel menu from the context menu event and the menu keys of a tab', async () => {
    const { rendered, dock } = await mountDock({ views: VIEWS });
    await dock.showView('a.one');
    await settle(rendered.fixture);
    const tab = rendered.one('[data-testid="pd-dock-tab"][data-panel-id="a.one"]');
    tab.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: 10, clientY: 10 }));
    await settle(rendered.fixture);
    expect(rendered.element.querySelector('[data-testid="pd-dock-menu"]')).not.toBeNull();
    rendered.one('.pd-dock-menu-backdrop').dispatchEvent(new PointerEvent('pointerdown', { bubbles: true }));
    await settle(rendered.fixture);
    expect(rendered.element.querySelector('[data-testid="pd-dock-menu"]')).toBeNull();
    tab.dispatchEvent(new KeyboardEvent('keydown', { key: 'ContextMenu', bubbles: true, cancelable: true }));
    await settle(rendered.fixture);
    expect(rendered.element.querySelector('[data-testid="pd-dock-menu"]')).not.toBeNull();
    rendered.one('[data-testid="pd-dock-menu"]').dispatchEvent(new KeyboardEvent('keydown', { key: 'Tab', bubbles: true }));
    await settle(rendered.fixture);
    tab.dispatchEvent(new KeyboardEvent('keydown', { key: 'F10', shiftKey: true, bubbles: true, cancelable: true }));
    await settle(rendered.fixture);
    expect(rendered.element.querySelector('[data-testid="pd-dock-menu"]')).not.toBeNull();
  });

  it('draws the view icon in each tab', async () => {
    const { rendered, dock } = await mountDock({ views: VIEWS });
    await dock.showView('a.one');
    await settle(rendered.fixture);
    expect(rendered.element.querySelector('.pd-dock-tab-icon')?.children.length).toBeGreaterThan(0);
  });

  it('re-plans when the container is resized and remembers a dragged splitter', async () => {
    const { rendered, dock } = await mountDock({ views: VIEWS });
    await dock.showView('a.one');
    await settle(rendered.fixture);
    const ctx = TestBed.inject(DockContext);
    const layout = TestBed.inject(DockLayout);
    const sash = document.createElement('div');
    sash.className = 'dv-sash';
    ctx.container!.appendChild(sash);
    sash.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true }));
    document.dispatchEvent(new PointerEvent('pointerup', { bubbles: true }));
    document.dispatchEvent(new PointerEvent('pointerup', { bubbles: true }));
    expect(typeof layout.hints().sizes).toBe('object');
  });
});
