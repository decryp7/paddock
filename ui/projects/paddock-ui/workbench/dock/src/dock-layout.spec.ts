import { provideZonelessChangeDetection } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { defaultViewLayout, type ViewLayout } from '@decryp7/paddock-ui';
import { msg } from '@decryp7/paddock-ui/shell';
import { defineView, provideWorkbenchContributions } from '@decryp7/paddock-ui/workbench';
import type { DockviewApi } from 'dockview';
import { DockContext } from './dock-context';
import { DockLayout } from './dock-layout';
import { DockViews } from './dock-views';
import { viewLayout, viewRow } from './view-layout';

const load = () => Promise.resolve(class {});
const layout = (area: ViewLayout['area'], min: [number, number], pref: [number, number], priority: number, edge?: 'left' | 'right'): ViewLayout => ({
  area,
  min: { width: min[0], height: min[1] },
  pref: { width: pref[0], height: pref[1] },
  priority,
  ...(edge ? { edge } : {}),
});
const view = (id: string, l: ViewLayout, extra: { fullHeight?: boolean; instances?: boolean } = {}) => defineView({ id, title: msg(`t.${id.replace('#', '-')}`, id), icon: 'pd:x', layout: l, load, ...extra });

/** Views shaped like a small IDE: two documents, a full-height Home, an assistant at the right edge. */
const VIEWS = [
  view('timeline', layout('document', [480, 200], [900, 300], 100), { instances: true }),
  view('events', layout('document', [560, 240], [900, 400], 110)),
  view('home', layout('document', [420, 400], [700, 600], 60), { fullHeight: true }),
  view('ai-chat', layout('side', [320, 240], [372, 400], 70, 'right')),
  view('problems', layout('analysis', [420, 160], [800, 240], 55)),
];

interface FakeGroup {
  id: string;
  width: number;
  height: number;
  panels: { id: string }[];
  element: HTMLElement;
  api: { location: { type: string }; setConstraints: ReturnType<typeof vi.fn>; setSize: ReturnType<typeof vi.fn> };
}

/** A fake dock: one group per panel id (ids joined by "+" share a group), with sizes. */
function fakeDock(width: number, height: number, spec: Record<string, { width: number; height: number }>) {
  const groups: FakeGroup[] = [];
  const panels = new Map<string, { id: string; group: FakeGroup; api: { moveTo: ReturnType<typeof vi.fn>; setActive: ReturnType<typeof vi.fn> } }>();
  for (const [ids, size] of Object.entries(spec)) {
    const group: FakeGroup = { id: `g${groups.length}`, ...size, panels: [], element: document.createElement('div'), api: { location: { type: 'grid' }, setConstraints: vi.fn(), setSize: vi.fn() } };
    groups.push(group);
    for (const id of ids.split('+')) {
      group.panels.push({ id });
      panels.set(id, { id, group, api: { moveTo: vi.fn(), setActive: vi.fn() } });
    }
  }
  const api = { width, height, groups, panels: [...panels.values()], getPanel: (id: string) => panels.get(id), removePanel: vi.fn() } as unknown as DockviewApi;
  return { api, groups, panels };
}

function setup() {
  TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection(), provideWorkbenchContributions({ views: VIEWS }), DockViews, DockContext, DockLayout] });
  return TestBed.inject(DockLayout);
}

describe('view layouts', () => {
  it('a view that declares nothing gets its area\'s default (a panel whose view is gone)', () => {
    expect(viewLayout(undefined)).toEqual(defaultViewLayout('document'));
  });

  it('rows: analysis shares one row, every other view sits in the main row', () => {
    expect(viewRow({ layout: defaultViewLayout('analysis') })).toBe('analysis');
    expect(viewRow({ layout: defaultViewLayout('document') })).toBe('main');
  });
});

describe('DockLayout.fit', () => {
  it('holds a group to the window when the window is smaller than its views\' minimum (320 px; review P1-2)', () => {
    const layout = setup();
    const { api, groups } = fakeDock(272, 200, { events: { width: 272, height: 200 } });
    layout.constrain(api);
    expect(groups[0]!.api.setConstraints).toHaveBeenCalledWith({ minimumWidth: 272, minimumHeight: 200 });
    expect(groups[0]!.element.dataset['minWidth']).toBe('560');
  });

  it('constrains every group from the views it holds (events 560, assistant 320), also without a measured size', () => {
    const layout = setup();
    const { api, groups } = fakeDock(0, 0, { events: { width: 0, height: 0 }, 'ai-chat': { width: 0, height: 0 } });
    expect(layout.fit(api)).toBeNull();
    expect(groups[0]!.api.setConstraints).toHaveBeenCalledWith({ minimumWidth: 560, minimumHeight: 240 });
    expect(groups[1]!.api.setConstraints).toHaveBeenCalledWith({ minimumWidth: 320 });
    // The declared minimum is readable from outside (the scenario tests assert against it).
    expect(groups[0]!.element.dataset['minWidth']).toBe('560');
    expect(groups[1]!.element.dataset['minWidth']).toBe('320');
  });

  it('tabs Home into the events group when the window has no room for it, and never squeezes the assistant', () => {
    const layout = setup();
    const { api, panels, groups } = fakeDock(1100, 800, { 'timeline+events': { width: 400, height: 800 }, 'ai-chat': { width: 372, height: 800 }, home: { width: 328, height: 800 } });
    const plan = layout.fit(api, { sizes: true, fresh: ['home'] })!;
    expect(plan.tabbed).toEqual([{ id: 'home', into: 'events' }]);
    expect(panels.get('home')!.api.moveTo).toHaveBeenCalledWith({ group: groups[0], position: 'center' });
    expect(panels.get('home')!.api.setActive).toHaveBeenCalled();
    expect(plan.violations).toEqual([]);
  });

  it('a view opened in the background (auto-open) that has no room never takes the front from the view it joins', () => {
    const layout = setup();
    const { api, panels, groups } = fakeDock(1100, 800, { 'timeline+events': { width: 400, height: 800 }, 'ai-chat': { width: 372, height: 800 }, home: { width: 328, height: 800 } });
    const eventsActive = { api: { setActive: vi.fn() } };
    (groups[0] as unknown as { activePanel: unknown }).activePanel = eventsActive;
    layout.fit(api, { sizes: true, fresh: ['home'], background: true });
    expect(panels.get('home')!.api.moveTo).toHaveBeenCalled();
    expect(panels.get('home')!.api.setActive).not.toHaveBeenCalled();
    expect(eventsActive.api.setActive).toHaveBeenCalled();
  });

  it('a view that yields without being opened just now joins as a background tab: the tab in front stays in front', () => {
    const layout = setup();
    const { api, panels, groups } = fakeDock(1100, 800, { 'timeline+events': { width: 400, height: 800 }, 'ai-chat': { width: 372, height: 800 }, home: { width: 328, height: 800 } });
    const eventsActive = { api: { setActive: vi.fn() } };
    (groups[0] as unknown as { activePanel: unknown }).activePanel = eventsActive;
    layout.fit(api, { sizes: true });
    expect(panels.get('home')!.api.setActive).not.toHaveBeenCalled();
    expect(eventsActive.api.setActive).toHaveBeenCalled();
  });

  it('gives a new Home column its preferred width at 1920 and shrinks only what the plan says at 1440', () => {
    const layout = setup();
    const wide = fakeDock(1920, 1080, { 'timeline+events': { width: 1000, height: 1080 }, 'ai-chat': { width: 372, height: 1080 }, home: { width: 548, height: 1080 } });
    layout.fit(wide.api, { sizes: true, fresh: ['home'] });
    expect(wide.groups[2]!.api.setSize).toHaveBeenCalledWith({ width: 700 });
    const narrow = fakeDock(1440, 900, { 'timeline+events': { width: 520, height: 900 }, 'ai-chat': { width: 372, height: 900 }, home: { width: 548, height: 900 } });
    const plan = layout.fit(narrow.api, { sizes: true, fresh: ['home'] })!;
    expect(plan.tabbed).toEqual([]);
    expect(narrow.groups[2]!.api.setSize).toHaveBeenCalledWith({ width: 508 });
  });

  it('auto-hides a side tool to the right edge when there is no room left even after tabbing', () => {
    const layout = setup();
    const { api, groups } = fakeDock(800, 700, { 'timeline+events': { width: 428, height: 700 }, 'ai-chat': { width: 372, height: 700 } });
    const ctx = TestBed.inject(DockContext);
    ctx.api = api;
    const plan = layout.fit(api, { fresh: ['ai-chat'] })!;
    expect(plan.hidden).toEqual([{ id: 'ai-chat', edge: 'right' }]);
    expect(api.removePanel).toHaveBeenCalled();
    expect(ctx.autoHide().items.map((i) => i.id)).toEqual(['ai-chat']);
    expect(ctx.autoHide().open).toBe('ai-chat');
    expect(groups.length).toBe(2);
  });

  it('needsFit: false for a sound layout, true for a group under its minimum or a view that must yield', () => {
    const layout = setup();
    const ok = fakeDock(1440, 900, { 'timeline+events': { width: 1068, height: 900 }, 'ai-chat': { width: 372, height: 900 } });
    expect(layout.needsFit(ok.api, layout.plan(ok.api)!)).toBe(false);
    const small = fakeDock(1440, 900, { 'timeline+events': { width: 1180, height: 900 }, 'ai-chat': { width: 200, height: 900 } });
    expect(layout.needsFit(small.api, layout.plan(small.api)!)).toBe(true);
    const tight = fakeDock(800, 900, { 'timeline+events': { width: 428, height: 900 }, 'ai-chat': { width: 372, height: 900 } });
    expect(layout.needsFit(tight.api, layout.plan(tight.api)!)).toBe(true);
  });
});

describe('displaced views and grow-back', () => {
  const grid = () => fakeDock(1100, 800, { 'timeline+events': { width: 400, height: 800 }, 'ai-chat': { width: 372, height: 800 }, home: { width: 328, height: 800 } });

  it('records where a view was and what it joined when the plan tabs it away', () => {
    const layout = setup();
    const { api } = grid();
    const slot = { referencePanel: null, direction: 'within' as const, edge: 'right' as const, size: 700 };
    layout.fit(api, { sizes: true, capture: (id) => (id === 'home' ? slot : null) });
    expect(TestBed.inject(DockContext).displaced.get('home')).toEqual({ id: 'home', slot, into: 'events' });
  });

  it('records a view opened just now as a guest of its host too, so the next plan does not take the host for a full-height column', () => {
    const layout = setup();
    const { api, groups } = grid();
    layout.fit(api, { sizes: true, fresh: ['home'] });
    expect(TestBed.inject(DockContext).displaced.get('home')?.into).toBe('events');
    // The panel now lives in the events group (as dockview would have moved it): planning again leaves the timeline alone.
    groups[0]!.panels.push({ id: 'home' });
    expect(layout.plan(api)!.tabbed.map((t) => t.id)).toEqual(['home']);
  });

  it('instances of a view share its row: in separate groups they sit side by side in it', () => {
    const layout = setup();
    const { api } = fakeDock(1440, 1000, { timeline: { width: 720, height: 1000 }, 'timeline#2': { width: 720, height: 1000 } });
    const plan = layout.plan(api)!;
    expect(plan.violations).toEqual([]);
    expect(plan.tabbed).toEqual([]);
    expect(plan.columns[0]!.rows.map((r) => r.key)).toEqual(['main']);
  });

  it('regression: three documents tabbed in one group leave the analysis row its room (instances do not stack rows)', () => {
    const layout = setup();
    const { api } = fakeDock(820, 834, { 'events+timeline#a+timeline#b': { width: 820, height: 594 }, problems: { width: 820, height: 240 } });
    const plan = layout.plan(api)!;
    expect(plan.tabbed).toEqual([]);
    expect(plan.columns[0]!.rows.map((r) => r.key).sort()).toEqual(['analysis', 'main']);
  });

  it('records an auto-hidden side tool with no host', () => {
    const layout = setup();
    const { api } = fakeDock(800, 700, { 'timeline+events': { width: 428, height: 700 }, 'ai-chat': { width: 372, height: 700 } });
    TestBed.inject(DockContext).api = api;
    layout.fit(api);
    expect(TestBed.inject(DockContext).displaced.get('ai-chat')?.into).toBeNull();
  });

  it('a displaced view is a grow-back candidate only once there is room plus the margin, so it cannot flap', () => {
    const layout = setup();
    const ctx = TestBed.inject(DockContext);
    // Home tabbed into events; the dock is as wide as the threshold (1300) and then margin wider (1364).
    for (const [width, expected] of [[1300, []], [1363, []], [1364, ['home']]] as const) {
      const { api } = fakeDock(width, 900, { 'timeline+events+home': { width: 900, height: 900 }, 'ai-chat': { width: 372, height: 900 } });
      ctx.displaced.clear();
      ctx.displaced.set('home', { id: 'home', slot: { referencePanel: null, direction: 'within' }, into: 'events' });
      expect(layout.growBackIds(api), `${width}`).toEqual([...expected]);
    }
  });

  it('restoreOrder puts a view after the view its slot is relative to, and survives a cycle', () => {
    const layout = setup();
    const d = TestBed.inject(DockContext).displaced;
    const at = (id: string, ref: string | null) => d.set(id, { id, slot: { referencePanel: ref, direction: 'within' }, into: null });
    at('home', 'registry');
    at('registry', 'ai-chat');
    at('ai-chat', null);
    expect(layout.restoreOrder(['home', 'registry', 'ai-chat'])).toEqual(['ai-chat', 'registry', 'home']);
    at('a', 'b');
    at('b', 'a');
    expect(layout.restoreOrder(['a', 'b'])).toEqual(['a', 'b']);
  });

  it('a view tabbed in by the plan does not turn its host into a full-height column', () => {
    const layout = setup();
    const { api } = fakeDock(1500, 900, { 'timeline+events+home': { width: 1000, height: 900 }, 'ai-chat': { width: 372, height: 900 } });
    TestBed.inject(DockContext).displaced.set('home', { id: 'home', slot: { referencePanel: null, direction: 'within' }, into: 'events' });
    const open = layout.openViews(api);
    expect(open.find((v) => v.id === 'events')?.fullHeight).toBeUndefined();
    expect(open.find((v) => v.id === 'home')?.fullHeight).toBe(true);
  });

  it('moveBack: a column at the window edge is re-added by the caller, a neighbour slot is a move, a moved or missing view is stale', () => {
    const layout = setup();
    const { api, panels } = fakeDock(1500, 900, { 'timeline+events+home': { width: 1100, height: 900 }, 'ai-chat': { width: 372, height: 900 } });
    expect(layout.moveBack(api, { id: 'home', slot: { referencePanel: null, direction: 'within', edge: 'right' }, into: 'events' })).toBe('readd');
    expect(layout.moveBack(api, { id: 'home', slot: { referencePanel: 'ai-chat', direction: 'left' }, into: 'events' })).toBe('moved');
    expect(panels.get('home')!.api.moveTo).toHaveBeenCalledWith({ group: panels.get('ai-chat')!.group, position: 'left' });
    expect(layout.moveBack(api, { id: 'home', slot: { referencePanel: null, direction: 'within' }, into: 'ai-chat' })).toBe('stale');
    expect(layout.moveBack(api, { id: 'gone', slot: { referencePanel: null, direction: 'within' }, into: 'events' })).toBe('stale');
  });

  it('remembers dragged sizes as the user\'s wish and uses them (never below the minimum) as hints', () => {
    const layout = setup();
    const ctx = TestBed.inject(DockContext);
    const { api } = fakeDock(1920, 1080, { 'timeline+events': { width: 1200, height: 1080 }, 'ai-chat': { width: 500, height: 1080 } });
    layout.rememberSizes(api);
    expect(ctx.userSizes['ai-chat']).toEqual({ width: 500, height: 1080 });
    expect(layout.hints().sizes?.['ai-chat']?.width).toBe(500);
  });

  it('the Explorer verdict: hides when the plan has no room, shows again only with the margin', () => {
    const layout = setup();
    const narrow = fakeDock(400, 900, { 'timeline+events': { width: 400, height: 900 } });
    expect(layout.explorerVerdict(narrow.api, true, 300)).toBe('hide');
    const roomy = fakeDock(1200, 900, { 'timeline+events': { width: 1200, height: 900 } });
    expect(layout.explorerVerdict(roomy.api, true, 300)).toBe('keep');
    // Closed: the dock has the Explorer's room too; it needs 300 + 560 + the margin.
    expect(layout.explorerVerdict(fakeDock(923, 900, { 'timeline+events': { width: 923, height: 900 } }).api, false, 300)).toBe('keep');
    expect(layout.explorerVerdict(fakeDock(925, 900, { 'timeline+events': { width: 925, height: 900 } }).api, false, 300)).toBe('show');
  });
});

describe('watchResize', () => {
  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  /** A container whose size the test sets, and a ResizeObserver it fires by hand. */
  function resizing(apiSize: { width: number; height: number }) {
    let fire: () => void = () => undefined;
    vi.stubGlobal(
      'ResizeObserver',
      class {
        constructor(cb: () => void) {
          fire = cb;
        }
        observe(): void {}
        disconnect(): void {}
      },
    );
    const size = { width: 1440, height: 800 };
    const container = document.createElement('div');
    container.getBoundingClientRect = () => ({ ...size, x: 0, y: 0, top: 0, left: 0, right: size.width, bottom: size.height, toJSON: () => ({}) }) as DOMRect;
    const layout = setup();
    const ctx = TestBed.inject(DockContext);
    const api = { ...apiSize } as unknown as DockviewApi;
    ctx.container = container;
    ctx.api = api;
    return { layout, api, size, fire: () => fire() };
  }

  it('plans once dockview has measured the new size, not before (a plan on the old size is never corrected)', () => {
    vi.useFakeTimers();
    const { layout, api, size, fire } = resizing({ width: 1440, height: 800 });
    const onResize = vi.fn();
    layout.watchResize(api, onResize);
    size.width = 900;
    fire();
    vi.advanceTimersByTime(150);
    expect(onResize).not.toHaveBeenCalled(); // dockview still reports 1440
    vi.advanceTimersByTime(100);
    expect(onResize).not.toHaveBeenCalled();
    (api as { width: number }).width = 900;
    vi.advanceTimersByTime(50);
    expect(onResize).toHaveBeenCalledTimes(1);
  });

  it('still plans when dockview never agrees (bounded wait)', () => {
    vi.useFakeTimers();
    const { layout, api, size, fire } = resizing({ width: 1440, height: 800 });
    const onResize = vi.fn();
    layout.watchResize(api, onResize);
    size.width = 900;
    fire();
    vi.advanceTimersByTime(150 + 50 * 20 + 50);
    expect(onResize).toHaveBeenCalledTimes(1);
  });
});

describe('DockLayout: sizes of cells and rows, edge cases', () => {
  it('sets the planned width of side-by-side groups and the planned height of a row, and leaves groups already at size alone', () => {
    const layout = setup();
    const { api, groups } = fakeDock(1900, 1000, { timeline: { width: 100, height: 100 }, events: { width: 100, height: 100 }, problems: { width: 1900, height: 10 } });
    layout.fit(api, { sizes: true });
    expect(groups[0]!.api.setSize).toHaveBeenCalledWith({ width: expect.any(Number) });
    // events is the anchor of its row: it takes what the others leave.
    expect(groups[1]!.api.setSize).not.toHaveBeenCalled();
    expect(groups[2]!.api.setSize).toHaveBeenCalledWith({ height: expect.any(Number) });
    const settled = groups.map((g) => g.api.setSize.mock.calls.length);
    for (const g of groups) {
      const planned = g.api.setSize.mock.calls.at(-1)?.[0] as { width?: number; height?: number } | undefined;
      if (!planned) continue;
      if (planned.width !== undefined) g.width = planned.width;
      if (planned.height !== undefined) g.height = planned.height;
    }
    layout.fit(api, { sizes: true });
    expect(groups.map((g) => g.api.setSize.mock.calls.length)).toEqual(settled);
  });

  it('ignores a planned view whose group is not in the grid (floating) when applying sizes', () => {
    const layout = setup();
    const { api, groups } = fakeDock(1500, 900, { timeline: { width: 100, height: 900 }, 'ai-chat': { width: 372, height: 900 } });
    for (const g of groups) g.api.location.type = 'floating';
    layout.fit(api, { sizes: true });
    for (const g of groups) expect(g.api.setSize).not.toHaveBeenCalled();
  });

  it('skips a planned view whose panel is gone by the time sizes are applied', () => {
    const layout = setup();
    const { api, groups } = fakeDock(1500, 900, { 'timeline+events': { width: 100, height: 900 }, 'ai-chat': { width: 100, height: 900 } });
    const plan = layout.plan(api)!;
    vi.spyOn(layout, 'plan').mockReturnValue(plan);
    (api as { getPanel: unknown }).getPanel = () => undefined;
    layout.fit(api, { sizes: true });
    for (const g of groups) expect(g.api.setSize).not.toHaveBeenCalled();
  });

  it('moveBack re-adds a view whose slot has no neighbour, or whose neighbour sits in the same group', () => {
    const layout = setup();
    const { api } = fakeDock(1500, 900, { 'timeline+events+home': { width: 1100, height: 900 } });
    expect(layout.moveBack(api, { id: 'home', slot: { referencePanel: null, direction: 'within' }, into: 'events' })).toBe('readd');
    expect(layout.moveBack(api, { id: 'home', slot: { referencePanel: 'timeline', direction: 'left' }, into: 'events' })).toBe('readd');
  });
});

describe('DockLayout.whenSized', () => {
  afterEach(() => vi.unstubAllGlobals());

  function observed() {
    let fire: () => void = () => undefined;
    const disconnect = vi.fn();
    vi.stubGlobal(
      'ResizeObserver',
      class {
        constructor(cb: () => void) {
          fire = cb;
        }
        observe(): void {}
        disconnect = disconnect;
      },
    );
    const layout = setup();
    const ctx = TestBed.inject(DockContext);
    ctx.container = document.createElement('div');
    return { layout, ctx, fire: () => fire(), disconnect };
  }

  it('applies at once when the dock is measured, or when there is no container to wait on', () => {
    const { layout, ctx } = observed();
    const apply = vi.fn();
    layout.whenSized({ width: 10, height: 10 } as unknown as DockviewApi, apply);
    expect(apply).toHaveBeenCalledTimes(1);
    ctx.container = null;
    layout.whenSized({ width: 0, height: 0 } as unknown as DockviewApi, apply);
    expect(apply).toHaveBeenCalledTimes(2);
  });

  it('waits for the first measured size, then applies once', () => {
    const { layout, ctx, fire, disconnect } = observed();
    const api = { width: 0, height: 0 } as { width: number; height: number };
    ctx.api = api as unknown as DockviewApi;
    const apply = vi.fn();
    layout.whenSized(api as unknown as DockviewApi, apply);
    fire();
    expect(apply).not.toHaveBeenCalled();
    api.width = 800;
    api.height = 600;
    fire();
    expect(apply).toHaveBeenCalledTimes(1);
    expect(disconnect).toHaveBeenCalled();
  });

  it('drops the work when the dock was replaced while it waited', () => {
    const { layout, ctx, fire } = observed();
    const api = { width: 0, height: 0 } as { width: number; height: number };
    ctx.api = api as unknown as DockviewApi;
    const apply = vi.fn();
    layout.whenSized(api as unknown as DockviewApi, apply);
    ctx.api = null;
    api.width = 800;
    api.height = 600;
    fire();
    expect(apply).not.toHaveBeenCalled();
  });
});
