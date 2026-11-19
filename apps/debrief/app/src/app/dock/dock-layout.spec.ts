import { TestBed } from '@angular/core/testing';
import type { DockviewApi } from 'dockview';
import { provideDebriefBuiltins } from '../builtin/provide-builtins';
import { BUILTIN_VIEWS } from '../builtin/builtin-views';
import { HOME_VIEWS } from '../home/home.plugin';
import { provideTinyBackendBare, tinyBackend } from '../../testing/helpers';
import { provideWorkspace } from '../core/workspace/provide-workspace';
import { DebriefView } from '../extensions/tokens';
import { defaultViewLayout } from '../layout-plan/layout-plan';
import { DockContext } from './dock-context';
import { DockLayout } from './dock-layout';
import { viewLayout, viewRow } from './view-layout';

const AI: DebriefView = { id: 'ai-chat', title: 'AI', icon: 'puzzle', defaultPosition: 'right', layout: { area: 'side', min: { width: 320, height: 240 }, pref: { width: 372, height: 400 }, priority: 70 }, component: async () => class {} };

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
  TestBed.configureTestingModule({
    providers: [provideDebriefBuiltins([...BUILTIN_VIEWS, ...HOME_VIEWS, AI]), provideWorkspace(), ...provideTinyBackendBare(tinyBackend())],
  });
  return TestBed.inject(DockLayout);
}

describe('view layouts', () => {
  it('every built-in and core view declares a layout with min <= pref and a priority', () => {
    for (const v of [...BUILTIN_VIEWS, ...HOME_VIEWS, AI]) {
      expect(v.layout, v.id).toBeDefined();
      expect(v.layout!.min.width, v.id).toBeLessThanOrEqual(v.layout!.pref.width);
      expect(v.layout!.min.height, v.id).toBeLessThanOrEqual(v.layout!.pref.height);
      expect(v.layout!.priority, v.id).toBeGreaterThan(0);
    }
  });

  it('a view that declares nothing gets its area\'s default', () => {
    expect(viewLayout({ id: 'x', defaultPosition: 'center' })).toEqual(defaultViewLayout('document'));
    expect(viewLayout({ id: 'inspector-like', defaultPosition: 'center', area: 'side' }).min.width).toBe(320);
    expect(viewLayout(undefined)).toEqual(defaultViewLayout('document'));
  });

  it('rows: analysis shares one row, events and timeline have their own, other documents sit beside the timeline', () => {
    expect(viewRow({ id: 'markers', layout: defaultViewLayout('analysis') })).toBe('analysis');
    expect(viewRow({ id: 'events' })).toBe('events');
    expect(viewRow({ id: 'metrics' })).toBe('timeline');
  });
});

describe('DockLayout.fit', () => {
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

  it('a second timeline is stacked below the first in a row of its own, never side by side with it', () => {
    const layout = setup();
    const { api } = fakeDock(1440, 1000, { timeline: { width: 1440, height: 300 }, 'timeline#2': { width: 1440, height: 300 }, events: { width: 1440, height: 400 } });
    const plan = layout.plan(api)!;
    expect(plan.violations).toEqual([]);
    expect(plan.tabbed).toEqual([]);
    expect(plan.columns[0]!.rows.map((r) => r.key).sort()).toEqual(['events', 'timeline', 'timeline#2']);
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
    // Closed (and not drawn): the dock has the Explorer's room too; it needs 300 + 560 + the margin.
    expect(layout.explorerVerdict(fakeDock(923, 900, { 'timeline+events': { width: 923, height: 900 } }).api, false, 0)).toBe('keep');
    expect(layout.explorerVerdict(fakeDock(925, 900, { 'timeline+events': { width: 925, height: 900 } }).api, false, 0)).toBe('show');
  });

  it('the Explorer verdict is the same before and after the render that draws it: the room is the container as laid out now plus what the Explorer takes now', () => {
    // debrief.yml run 8297: Ctrl+B opened the Explorer while a re-plan was due. The re-plan ran before the render (dock still 800 px, Explorer
    // not drawn) or before dockview followed its container (dockview 800 px, container 527 px beside a drawn Explorer); adding the Explorer's width
    // to dockview's last size made the plan see 1100 px, say "keep", and the next re-plan closed the Explorer the user had just opened.
    const layout = setup();
    const ctx = TestBed.inject(DockContext);
    const container = document.createElement('div');
    let width = 0;
    vi.spyOn(container, 'getBoundingClientRect').mockImplementation(() => ({ width, height: 900 }) as DOMRect);
    ctx.container = container;
    const verdict = (room: number, drawn: number, open: boolean) => {
      const { api } = fakeDock(room, 900, { 'timeline+events': { width: room, height: 900 } });
      width = room - drawn;
      return layout.explorerVerdict(api, open, drawn);
    };
    // 800 px of room: the Explorer (min 300) and the events (min 560) do not fit, drawn or not yet drawn.
    expect(verdict(800, 0, true)).toBe('hide');
    expect(verdict(800, 273, true)).toBe('hide');
    // 1300 px of room: they fit, drawn or not.
    expect(verdict(1300, 0, true)).toBe('keep');
    expect(verdict(1300, 273, true)).toBe('keep');
    // Closed but still drawn (the render that removes it has not run): the same room, so the same answer as closed and gone.
    expect(verdict(1300, 273, false)).toBe(verdict(1300, 0, false));
    expect(verdict(800, 273, false)).toBe('keep');
  });
});

describe('the dock\'s room', () => {
  /** A dockview whose own size the decisions must not read: every read of `width` or `height` is recorded. */
  function untouchable(width: number, height: number) {
    const { api } = fakeDock(width, height, { 'timeline+events': { width, height } });
    const reads: string[] = [];
    for (const k of ['width', 'height'] as const) Object.defineProperty(api, k, { get: () => (reads.push(k), k === 'width' ? width : height) });
    return { api, reads };
  }

  function withContainer(width: number, height: number) {
    const layout = setup();
    const ctx = TestBed.inject(DockContext);
    const container = document.createElement('div');
    vi.spyOn(container, 'getBoundingClientRect').mockImplementation(() => ({ width, height }) as DOMRect);
    ctx.container = container;
    return layout;
  }

  it('is the container as laid out now, null while it has no box', () => {
    const layout = setup();
    const ctx = TestBed.inject(DockContext);
    const size = { width: 900, height: 700 };
    const container = document.createElement('div');
    vi.spyOn(container, 'getBoundingClientRect').mockImplementation(() => ({ ...size }) as DOMRect);
    ctx.container = container;
    const { api, reads } = untouchable(1440, 800);
    expect(layout.room(api)).toEqual({ width: 900, height: 700 });
    size.width = 0;
    expect(layout.room(api)).toBeNull();
    expect(reads).toEqual([]);
  });

  it('fit lays dockview out at the room before it sets any size (a view added while dockview still has its last size: addView)', () => {
    const layout = setup();
    const ctx = TestBed.inject(DockContext);
    const container = document.createElement('div');
    vi.spyOn(container, 'getBoundingClientRect').mockReturnValue({ width: 800, height: 900 } as DOMRect);
    ctx.container = container;
    const dock = fakeDock(1440, 900, { 'timeline+events': { width: 1440, height: 900 }, 'ai-chat': { width: 400, height: 900 } });
    const calls: string[] = [];
    (dock.api as unknown as { layout: (w: number, h: number) => void }).layout = (w, h) => calls.push(`layout ${w}x${h}`);
    for (const g of dock.groups) g.api.setSize.mockImplementation(() => calls.push(`setSize ${g.id}`));
    layout.fit(dock.api, { sizes: true, fresh: ['ai-chat'] });
    expect(calls[0]).toBe('layout 800x900');
    expect(calls.filter((c) => c.startsWith('layout'))).toHaveLength(1);
  });

  it('is dockview\'s size only without a container (unit tests of the plan)', () => {
    expect(setup().room(fakeDock(1440, 800, {}).api)).toEqual({ width: 1440, height: 800 });
  });

  it('plan, grow-back, needsFit and the Explorer verdict read the room, never dockview\'s size (it can lag its container)', () => {
    const layout = withContainer(800, 900);
    const { api, reads } = untouchable(1440, 900);
    const plan = layout.plan(api);
    expect(plan).not.toBeNull();
    layout.needsFit(api, plan!);
    // A displaced view, so grow-back asks the plan (it returns early with none).
    TestBed.inject(DockContext).displaced.set('home', { id: 'home', slot: { referencePanel: null, direction: 'within' }, into: 'events' });
    layout.growBackIds(api);
    layout.explorerVerdict(api, true, 0);
    expect(reads).toEqual([]);
    // And the answers are the container's: 800 px has no room for the Explorer beside the events, 1440 would have.
    expect(layout.explorerVerdict(api, true, 0)).toBe('hide');
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
    // `layout` does nothing here: the tests below are about a dockview that has not followed its container yet.
    const api = { ...apiSize, layout: vi.fn() } as unknown as DockviewApi;
    ctx.container = container;
    ctx.api = api;
    return { layout, api, size, fire: () => fire() };
  }

  it('re-plans once after the burst, from the container, without waiting for dockview to agree (the plan reads the room, not dockview)', () => {
    vi.useFakeTimers();
    const { layout, api, size, fire } = resizing({ width: 1440, height: 800 });
    const onResize = vi.fn();
    layout.watchResize(api, onResize);
    size.width = 900;
    fire();
    vi.advanceTimersByTime(149);
    expect(onResize).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1);
    // dockview still reports 1440 (its own observer is late): the re-plan runs anyway, and it is laid out at the container's size first.
    expect(onResize).toHaveBeenCalledTimes(1);
    expect(api.layout).toHaveBeenLastCalledWith(900, 800);
  });

  it('says a re-plan is pending from the first resize until it has run, and not before', () => {
    vi.useFakeTimers();
    const { layout, api, size, fire } = resizing({ width: 1440, height: 800 });
    const pending: boolean[] = [];
    layout.watchResize(api, vi.fn(), (p) => pending.push(p));
    expect(pending).toEqual([]);
    size.width = 900;
    fire();
    expect(pending).toEqual([true]);
    vi.advanceTimersByTime(150);
    expect(pending).toEqual([true, false]);
  });

  it('followContainer lays dockview out at the container size when they differ (in the observer, in the same frame), and only then', () => {
    const { layout, api, size } = resizing({ width: 1440, height: 800 });
    layout.followContainer(api);
    expect(api.layout).not.toHaveBeenCalled();
    size.width = 900;
    layout.followContainer(api);
    expect(api.layout).toHaveBeenCalledWith(900, 800);
  });

  it('the resize observer follows the container before it waits to plan', () => {
    vi.useFakeTimers();
    const { layout, api, size, fire } = resizing({ width: 1440, height: 800 });
    layout.watchResize(api, vi.fn());
    size.width = 900;
    fire();
    expect(api.layout).toHaveBeenCalledWith(900, 800);
  });

  it('followContainer ignores a container that has no box yet', () => {
    const { layout, api, size } = resizing({ width: 1440, height: 800 });
    size.width = 0;
    layout.followContainer(api);
    expect(api.layout).not.toHaveBeenCalled();
  });
});
