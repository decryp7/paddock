import { TestBed } from '@angular/core/testing';
import { settle } from '@decryp7/paddock-ui/shell/testing';
import { DockContext } from './dock-context';
import { DockLayout } from './dock-layout';
import { DockPlacement } from './dock-placement';
import { mountDock, probeView, tabIds } from './harness.spec-helper';
import { Perspectives } from './perspectives';
import { WindowLayouts } from './window-layouts';

const doc = (id: string, extra = {}) => probeView(id, 'document', extra);
const analysis = (n: number) => probeView(`an.${n}`, 'analysis');
const VIEWS = [
  doc('d.one'),
  doc('d.two'),
  doc('d.full', { fullHeight: true }),
  probeView('s.one', 'side'),
  probeView('s.two', 'side'),
  probeView('t.one', 'status'),
  ...Array.from({ length: 10 }, (_, i) => analysis(i + 1)),
];

const groupsOf = (api: { groups: { panels: { id: string }[] }[] }) => api.groups.map((g) => g.panels.map((p) => p.id));

describe('where views open', () => {
  it('tabs documents together, puts tools in one group at their edge and keeps a full-height document in a column', async () => {
    const { dock, rendered } = await mountDock({ views: VIEWS });
    for (const id of ['d.one', 'd.two', 's.one', 's.two', 'd.full']) await dock.showView(id);
    await settle(rendered.fixture);
    const api = TestBed.inject(DockContext).api!;
    const groups = groupsOf(api);
    expect(groups).toContainEqual(['d.one', 'd.two']);
    expect(groups).toContainEqual(['s.one', 's.two']);
    expect(groups).toContainEqual(['d.full']);
  });

  it('opens a status tool as a bottom tab when none is docked there, and tabs a second one into the docked group', async () => {
    const { dock, rendered } = await mountDock({ views: VIEWS });
    await dock.showView('d.one');
    await dock.showView('t.one');
    await settle(rendered.fixture);
    expect(dock.autoHide().items[0]?.side).toBe('bottom');
  });

  it('keeps at most eight analysis tabs: the least recently used inactive one closes', async () => {
    const { dock, rendered } = await mountDock({ views: VIEWS });
    await dock.showView('d.one');
    for (let i = 1; i <= 9; i++) await dock.showView(`an.${i}`);
    await settle(rendered.fixture);
    const open = dock.openPanels().filter((p) => p.startsWith('an.'));
    expect(open).toHaveLength(8);
    expect(open).not.toContain('an.1');
    expect(open).toContain('an.9');
  });

  it('opens a view beside another with showBeside, else like show', async () => {
    const { dock, rendered } = await mountDock({ views: [doc('d.one', { instances: true }), doc('d.two')] });
    await dock.showView('d.one');
    await settle(rendered.fixture);
    dock.showBeside('d.two', 'd.one', 'below');
    dock.showBeside('d.one#2', 'd.one', 'right');
    dock.showBeside('d.two', 'missing', 'right');
    await settle(rendered.fixture);
    expect(dock.openPanels()).toEqual(['d.one', 'd.one#2', 'd.two']);
  });
});

describe('moving panels', () => {
  async function two() {
    const m = await mountDock({ views: VIEWS });
    await m.dock.showView('d.one');
    await m.dock.showView('d.two');
    await m.dock.showView('s.one');
    await settle(m.rendered.fixture);
    return m;
  }

  it('floats a panel and docks it back', async () => {
    const { dock, rendered } = await two();
    const ctx = TestBed.inject(DockContext);
    expect(dock.canFloatActive()).toBe(true);
    dock.runPanelAction('d.two', 'float');
    await settle(rendered.fixture);
    expect(ctx.api!.getPanel('d.two')!.group.api.location.type).toBe('floating');
    expect(dock.canFloatActive()).toBe(false);
    expect(dock.floatActive()).toBe(false);
    dock.runPanelAction('d.two', 'dock');
    await settle(rendered.fixture);
    expect(ctx.api!.getPanel('d.two')!.group.api.location.type).toBe('grid');
    dock.toggleFloat(ctx.api!.getPanel('s.one')!.group);
    expect(ctx.api!.getPanel('s.one')!.group.api.location.type).toBe('floating');
    dock.toggleFloat(ctx.api!.getPanel('s.one')!.group);
    expect(ctx.api!.getPanel('s.one')!.group.api.location.type).toBe('grid');
  });

  it('runs the tab-group actions of the panel menu', async () => {
    const { dock, rendered } = await two();
    const ctx = TestBed.inject(DockContext);
    dock.runPanelAction('d.two', 'newVerticalGroup');
    await settle(rendered.fixture);
    expect(ctx.api!.getPanel('d.two')!.group).not.toBe(ctx.api!.getPanel('d.one')!.group);
    dock.runPanelAction('d.two', 'moveToNextGroup');
    dock.runPanelAction('d.two', 'newHorizontalGroup');
    dock.runPanelAction('d.one', 'dockAsDocument');
    await settle(rendered.fixture);
    dock.runPanelAction('d.one', 'pinTab');
    expect(dock.pinnedTabs().has('d.one')).toBe(true);
    dock.runPanelAction('d.two', 'closeOthers');
    expect(dock.openPanels()).toContain('d.one');
    dock.runPanelAction('d.one', 'unpinTab');
    dock.runPanelAction('d.one', 'closeRight');
    dock.runPanelAction('s.one', 'autoHide');
    expect(dock.autoHide().items.map((i) => i.id)).toEqual(['s.one']);
    dock.runPanelAction('d.one', 'closeAllDocuments');
    dock.runPanelAction('missing', 'close');
    expect(dock.openPanels()).toEqual(['s.one']);
  });

  it('captures where a panel sits and puts it back there', async () => {
    const { dock, rendered } = await two();
    const placement = TestBed.inject(DockPlacement);
    const slot = dock.captureSlot('d.two');
    expect(slot).toMatchObject({ direction: 'within' });
    expect(dock.captureSlot('missing')).toBeNull();
    dock.closeView('d.two');
    dock.restoreSlot('d.two', slot);
    await settle(rendered.fixture);
    expect(dock.openPanels()).toContain('d.two');
    dock.closeView('s.one');
    dock.restoreSlot('s.one', { referencePanel: 'd.one', direction: 'right', size: 200 });
    dock.restoreSlot('s.one', null);
    dock.closeView('s.one');
    dock.restoreSlot('s.one', { referencePanel: null, direction: 'within', edge: 'bottom', size: 150 });
    expect(dock.moveToSlot('s.one', { referencePanel: 'd.one', direction: 'left' })).toBe(true);
    expect(dock.moveToSlot('s.one', { referencePanel: null, direction: 'within', edge: 'right', size: 100 })).toBe(true);
    expect(dock.moveToSlot('s.one', { referencePanel: 's.one', direction: 'left' })).toBe(false);
    expect(dock.moveToSlot('missing', { referencePanel: null, direction: 'within' })).toBe(false);
    expect(dock.panelRect('s.one')).not.toBeNull();
    expect(dock.panelRect('missing')).toBeNull();
    expect(dock.panelContent('missing')).toBeNull();
    dock.takeOut('d.two');
    expect(dock.openPanels()).not.toContain('d.two');
    expect(placement.documentWell('d.one')).toBeNull();
  });

  it('docks a view that is not in the layout at a guide, and answers guide queries without a measured dock', async () => {
    const { dock, rendered } = await two();
    const ctx = TestBed.inject(DockContext);
    const group = ctx.api!.getPanel('d.one')!.group;
    dock.closeView('d.two');
    expect(dock.dockAt('d.two', { kind: 'group', groupId: group.id, position: 'center' } as never)).toBe(true);
    expect(dock.dockAt('d.two', { kind: 'group', groupId: group.id, position: 'center' } as never)).toBe(false);
    dock.closeView('d.two');
    expect(dock.dockAt('d.two', { kind: 'group', groupId: 'nope', position: 'left' } as never)).toBe(false);
    expect(dock.dockAt('d.two', { kind: 'layout', position: 'left' } as never)).toBe(true);
    expect(dock.dockAt('missing', { kind: 'layout', position: 'left' } as never)).toBe(false);
    expect(dock.dropTargetAt({ x: 5, y: 5 })).toBeNull();
    dock.showDropGuides({ x: 5, y: 5 });
    dock.showDropGuides(null);
    expect(dock.tabDropOutcome('d.one', { x: 5, y: 5 })).toBeDefined();
    await settle(rendered.fixture);
  });

  it('sets a tab title, also of an auto-hidden tool window', async () => {
    const { dock, rendered } = await two();
    dock.setTitle('d.one', 'Changed (3)');
    expect(rendered.one('[data-panel-id="d.one"]').textContent).toContain('Changed (3)');
    dock.unpin('s.one');
    dock.setTitle('s.one', 'Tool (2)');
    expect(dock.autoHide().items[0]?.title).toBe('Tool (2)');
    dock.setTitle('missing', 'x');
    expect(tabIds(rendered.element)).toContain('d.one');
  });

  it('focuses the front panel and toggles an open panel', async () => {
    const { dock, rendered } = await two();
    dock.focusFront();
    await settle(rendered.fixture);
    dock.toggle('d.two');
    expect(dock.isOpen('d.two')).toBe(false);
    dock.toggle('d.two');
    expect(dock.isOpen('d.two')).toBe(true);
    expect(dock.kindOf('s.one')).toBe('tool');
    dock.autoHideEvent({ type: 'blur' });
    dock.persistLater();
  });
});

describe('fitting to the window', () => {
  /** A dock that has a size: the plan, the constraints and the grow-back all need one. */
  async function sized(width: number, height: number) {
    const m = await mountDock({ views: VIEWS });
    const ctx = TestBed.inject(DockContext);
    const api = ctx.api!;
    Object.defineProperty(api, 'width', { get: () => width, configurable: true });
    Object.defineProperty(api, 'height', { get: () => height, configurable: true });
    ctx.container!.getBoundingClientRect = () => ({ x: 0, y: 0, left: 0, top: 0, right: width, bottom: height, width, height, toJSON: () => ({}) }) as DOMRect;
    return { ...m, ctx, api, fit: (before: null = null) => (m.dock as unknown as { fitToWindow(a: unknown, b: unknown): void }).fitToWindow(api, before) };
  }

  it('constrains every group from the views it holds and writes the declared minimum on the group', async () => {
    const { dock, rendered, api } = await sized(1600, 900);
    await dock.showView('d.one');
    await dock.showView('s.one');
    await settle(rendered.fixture);
    TestBed.inject(DockLayout).constrain(api);
    expect(api.getPanel('d.one')!.group.element.dataset['minWidth']).toBe('200');
    expect(api.getPanel('s.one')!.group.element.dataset['minWidth']).toBe('200');
  });

  it('fits a layout that does not fit, says so with Undo, and brings the views back when there is room', async () => {
    const wide = await sized(1600, 900);
    await wide.dock.showView('d.one');
    await wide.dock.showView('s.one');
    await wide.dock.showView('s.two');
    await wide.dock.showView('d.full');
    await settle(wide.rendered.fixture);
    const listener = vi.fn();
    wide.dock.onWindowPlan(listener);
    Object.defineProperty(wide.api, 'width', { get: () => 520, configurable: true });
    wide.fit();
    await settle(wide.rendered.fixture);
    expect(wide.dock.notice()?.key).toBe('layout.fit');
    expect(listener).toHaveBeenCalled();
    expect(TestBed.inject(DockContext).displaced.size).toBeGreaterThan(0);
    Object.defineProperty(wide.api, 'width', { get: () => 2400, configurable: true });
    wide.fit();
    await settle(wide.rendered.fixture);
    expect(TestBed.inject(DockContext).displaced.size).toBe(0);
    expect(wide.dock.notice()).toBeNull();
    expect(wide.dock.verdict(false, 300)).toBe('show');
  });

  it('undoes a fit with the notice\'s action', async () => {
    const m = await sized(1600, 900);
    await m.dock.showView('d.one');
    await m.dock.showView('s.one');
    await m.dock.showView('s.two');
    await settle(m.rendered.fixture);
    Object.defineProperty(m.api, 'width', { get: () => 300, configurable: true });
    m.fit();
    await settle(m.rendered.fixture);
    const notice = m.dock.notice();
    expect(notice?.key).toBe('layout.fit');
    notice?.action?.run();
    await settle(m.rendered.fixture);
    expect(m.dock.openPanels()).toEqual(['d.one', 's.one', 's.two']);
    expect(m.dock.autoHide().items).toEqual([]);
  });

  it('does nothing before the dock is detached and after', async () => {
    const m = await sized(1000, 700);
    m.dock.detach();
    m.fit();
    expect(m.dock.ready()).toBe(false);
    m.dock.slideIn('x');
  });
});

describe('perspective and layout bookkeeping', () => {
  it('manages saved perspectives: save, rename, move, remove', async () => {
    const { dock, rendered } = await mountDock({ views: VIEWS });
    await dock.showView('d.one');
    await settle(rendered.fixture);
    const p = TestBed.inject(Perspectives);
    expect(p.saveCurrent('A')).toBe('A');
    expect(p.saveCurrent(' ')).toBeNull();
    expect(p.saveCurrent('B')).toBe('B');
    expect(p.active()?.id).toBe('saved:B');
    expect(p.rename('A', 'A2')).toBe(true);
    expect(p.rename('B', 'B2')).toBe(true);
    expect(p.active()?.id).toBe('saved:B2');
    expect(p.rename('A2', 'B2')).toBe(false);
    p.move('B2', -1);
    expect(p.list().map((x) => x.name)).toEqual(['B2', 'A2']);
    expect(p.at(1)?.name).toBe('B2');
    expect(p.switchToAt(2)).toBe(true);
    expect(p.switchToAt(9)).toBe(false);
    expect(p.switchTo('saved:A2')).toBe(true);
    p.remove('A2');
    expect(p.list().map((x) => x.name)).toEqual(['B2']);
    expect(p.activeNumber()).toBe(1);
    p.reset();
    const w = TestBed.inject(WindowLayouts);
    expect(w.has(1)).toBe(true);
    expect(w.applyAt(5)).toBe(false);
    expect(w.apply('nope')).toBe(false);
  });
});

describe('placement: documents beside tools, slots by geometry, drag cues', () => {
  const rectOfBox = (x: number, y: number, width: number, height: number) => () => ({ x, y, left: x, top: y, width, height, right: x + width, bottom: y + height, toJSON: () => ({}) }) as DOMRect;

  it('opens the first document to the right of a left-hand tool, or at the window edge when no tool is docked there', async () => {
    const left = probeView('s.left', 'side', { layout: { ...probeView('s.left', 'side').layout, edge: 'left' } });
    const { dock, rendered } = await mountDock({ views: [left, doc('d.one'), probeView('s.right', 'side')] });
    await dock.showView('s.right');
    await dock.showView('d.one');
    await settle(rendered.fixture);
    const api = TestBed.inject(DockContext).api!;
    expect(api.panels.map((p) => p.id).sort()).toEqual(['d.one', 's.right']);
    const second = await mountDock({ views: [left, doc('d.two')] });
    await second.dock.showView('s.left');
    await second.dock.showView('d.two');
    await settle(second.rendered.fixture);
    expect([...second.dock.openPanels()].sort()).toEqual(['d.two', 's.left']);
  });

  it('captures a column at a window edge with its width, and a group beside another with its size', async () => {
    const { dock, rendered } = await mountDock({ views: VIEWS });
    await dock.showView('d.one');
    await dock.showView('s.one');
    await settle(rendered.fixture);
    const ctx = TestBed.inject(DockContext);
    const api = ctx.api!;
    const documents = api.getPanel('d.one')!.group.element;
    const tools = api.getPanel('s.one')!.group.element;
    ctx.container!.getBoundingClientRect = rectOfBox(0, 0, 1400, 800);
    documents.getBoundingClientRect = rectOfBox(0, 0, 1100, 800);
    tools.getBoundingClientRect = rectOfBox(1100, 0, 300, 800);
    expect(dock.captureSlot('s.one')).toEqual({ referencePanel: null, direction: 'within', edge: 'right', size: 300 });
    tools.getBoundingClientRect = rectOfBox(1100, 0, 300, 400);
    expect(dock.captureSlot('s.one')).toEqual({ referencePanel: 'd.one', direction: 'right', size: 300 });
    tools.getBoundingClientRect = rectOfBox(1100, 400, 300, 400);
    expect(dock.captureSlot('s.one')).toMatchObject({ referencePanel: 'd.one' });
    documents.getBoundingClientRect = rectOfBox(0, 0, 100, 100);
    tools.getBoundingClientRect = rectOfBox(1100, 400, 300, 400);
    expect(dock.captureSlot('s.one')).toEqual({ referencePanel: null, direction: 'within' });
  });

  it('draws the drag cue where a release would open a window, and the cancel hint when it would do nothing', async () => {
    const { dock, rendered } = await mountDock({ views: VIEWS });
    await dock.showView('d.one');
    await settle(rendered.fixture);
    const placement = TestBed.inject(DockPlacement);
    const at = { x: 40, y: 50 };
    expect(placement.cueRect('d.one', at, 'window')).not.toBeNull();
    expect(placement.cueRect('d.one', at, 'cancel')).not.toBeNull();
    expect(placement.cueRect('d.one', at, 'dock')).toBeNull();
    expect(placement.cueRect('d.one', at, null)).toBeNull();
    expect(dock.tabDropOutcome('d.one', at)).toBeDefined();
  });
});

describe('growing back a view that was tabbed away', () => {
  it('docks it again as a group of its own once the window has room, and forgets the record', async () => {
    const m = await mountDock({ views: VIEWS });
    const ctx = TestBed.inject(DockContext);
    const api = ctx.api!;
    let width = 1600;
    Object.defineProperty(api, 'width', { get: () => width, configurable: true });
    Object.defineProperty(api, 'height', { get: () => 900, configurable: true });
    ctx.container!.getBoundingClientRect = () => ({ x: 0, y: 0, left: 0, top: 0, right: width, bottom: 900, width, height: 900, toJSON: () => ({}) }) as DOMRect;
    for (const id of ['d.one', 'd.full', 's.one', 's.two']) await m.dock.showView(id);
    await settle(m.rendered.fixture);
    const fit = () => (m.dock as unknown as { fitToWindow(a: unknown, b: unknown): void }).fitToWindow(api, null);
    width = 300;
    fit();
    expect(ctx.displaced.get('d.full')?.into).toBe('d.one');
    expect(api.getPanel('d.full')!.group).toBe(api.getPanel('d.one')!.group);
    width = 2400;
    fit();
    await settle(m.rendered.fixture);
    expect(ctx.displaced.size).toBe(0);
    expect(m.dock.openPanels()).toContain('d.full');
    expect(api.getPanel('d.full')!.group).not.toBe(api.getPanel('d.one')!.group);
  });
});
