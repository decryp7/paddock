import { LiveAnnouncer } from '@angular/cdk/a11y';
import { ChangeDetectionStrategy, Component, provideZonelessChangeDetection, signal } from '@angular/core';
import { TestBed, type ComponentFixture } from '@angular/core/testing';
import { MatIconRegistry } from '@angular/material/icon';
import { DomSanitizer } from '@angular/platform-browser';
import { registerPaddockIcons } from '@decryp7/paddock-ui/icons';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { Arrangement, Cells, Edit, KindInfo, Moves, PreviewRequest, Rect, SurfaceValues, WidgetState } from '../generated/widgets';
import { ManualFrameScheduler, PD_FRAME_SCHEDULER } from './frame-scheduler';
import { PD_WIDGET_GRID_BACKEND, type GridView, type PreviewReply, type WidgetGridBackend } from './grid-backend';
import { PdWidgetGrid } from './grid';
import { PdWidget } from './widget';

type W = { kind: string };
type V = { text: string };
type View = GridView<W, string, string>;

const R = (x: number, y: number, w: number, h: number): Rect => ({ x, y, w, h });
const kind = (k: string, title: string): KindInfo<string> => ({
  kind: k,
  title,
  summary: `${title} summary`,
  icon: 'pd:activity',
  size: { cells: { widths: { range: { min: 2, max: 12 } }, heights: { range: { min: 2, max: 14 } }, defaultW: 6, defaultH: 4 } },
  maxInstances: 1,
  refresh: 'live',
});
const place = (id: string, k: string, r: Rect): { id: string; widget: W } & Cells => ({ id, widget: { kind: k }, ...r });
const ALL: Moves = { left: true, right: true, up: true, down: true, first: true, last: true, wider: true, narrower: true, taller: true, shorter: true };

/** Three widgets (a, b, c) on a 12-column design arrangement, with a 6-column and a 1-column one. */
function mkView(over: Partial<View> = {}, revision = 7): View {
  const rects = { a: R(0, 0, 6, 4), b: R(6, 0, 6, 4), c: R(0, 4, 12, 4) };
  const arr = (minWidth: number, columns: number, rs: Record<string, Rect>): Arrangement => ({ minWidth, columns, placements: Object.entries(rs).map(([id, rect]) => ({ id, rect })) });
  return {
    layout: { version: 2, placements: [place('a', 'cpu', rects.a), place('b', 'net', rects.b), place('c', 'disk', rects.c)], presets: [] },
    revision,
    arrangements: [
      arr(840, 12, rects),
      arr(560, 6, { a: R(0, 0, 3, 4), b: R(3, 0, 3, 4), c: R(0, 4, 6, 4) }),
      arr(0, 1, { a: R(0, 0, 1, 4), b: R(0, 4, 1, 4), c: R(0, 8, 1, 4) }),
    ],
    moves: [
      { id: 'a', moves: { ...ALL, left: false } },
      { id: 'b', moves: { ...ALL, right: false } },
      { id: 'c', moves: ALL },
    ],
    canRedo: false,
    outcome: null,
    catalogue: { surface: { id: 'dashboard', noun: 'dashboard', columns: 12, maxPlacements: 32, minPlacements: 0, maxUserPresets: 3 }, kinds: [kind('cpu', 'CPU'), kind('net', 'Network'), kind('disk', 'Disk')] },
    builtinPresets: [],
    notes: [],
    canUndo: false,
    editable: true,
    ...over,
  };
}

/** The view after `a` moved to the right of `b`: the order b, a, c. */
function moved(revision = 8): View {
  const v = mkView({ canUndo: true }, revision);
  const rects = { b: R(0, 0, 6, 4), a: R(6, 0, 6, 4), c: R(0, 4, 12, 4) };
  return { ...v, arrangements: [{ minWidth: 840, columns: 12, placements: Object.entries(rects).map(([id, rect]) => ({ id, rect })) }, ...v.arrangements.slice(1)] };
}

class FakeBackend implements WidgetGridBackend<W, string, string> {
  preview = vi.fn<(r: PreviewRequest) => Promise<PreviewReply<W, string, string>>>();
  edit = vi.fn<(e: Edit<W>, revision: number) => Promise<View>>();
  reload = vi.fn<() => Promise<View>>();
}

const ready = (text: string): WidgetState<V, string> => ({ state: 'ready', value: { text }, asOf: 1, staleSince: null });

@Component({
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [PdWidgetGrid, PdWidget],
  template: `
    <pd-widget-grid
      [view]="view()"
      [error]="error()"
      [values]="values()"
      [width]="width()"
      [meta]="meta"
      [scroller]="null"
      [(editing)]="editing"
      (viewChange)="changes.push($event)"
      (action)="actions.push($event)"
      [hasOptions]="hasOptions"
      (options)="opened = $event"
      (addWidget)="adds = adds + 1"
      (retry)="retries = retries + 1"
    >
      <ng-template pdWidget="cpu" [pdWidgetOf]="w" let-v>{{ v?.text }}</ng-template>
    </pd-widget-grid>
  `,
})
class Host {
  readonly w: V | null = null;
  readonly view = signal<View | null>(mkView());
  readonly error = signal<string | null>(null);
  readonly values = signal<SurfaceValues<V, string> | null>({ notice: null, placements: [{ id: 'a', state: ready('cpu text') }] });
  readonly width = signal<number | null>(1200);
  readonly editing = signal(false);
  readonly meta = (id: string) => (id === 'a' ? '3 volumes' : null);
  changes: View[] = [];
  actions: string[] = [];
  adds = 0;
  retries = 0;
  opened = '';
  readonly hasOptions = (k: string) => k === 'cpu';
}

async function settle(f: { whenStable(): Promise<unknown> }): Promise<void> {
  await f.whenStable();
  TestBed.tick();
  await f.whenStable();
  await Promise.resolve();
  await f.whenStable();
}

const rect = (left: number, top: number, width: number, height: number) => ({ left, top, width, height, right: left + width, bottom: top + height, x: left, y: top, toJSON: () => ({}) }) as DOMRect;

describe('PdWidgetGrid', () => {
  let backend: FakeBackend;
  let sched: ManualFrameScheduler;
  let f: ComponentFixture<Host>;
  let say: ReturnType<typeof vi.spyOn>;
  let hostWidth: number;

  const q = (sel: string) => f.nativeElement.querySelector(sel) as HTMLElement | null;
  const qa = (sel: string) => [...(f.nativeElement as HTMLElement).querySelectorAll<HTMLElement>(sel)];
  const cell = (id: string) => q(`[data-pd-cell="${id}"]`) as HTMLElement;
  const ids = () => qa('[data-pd-cell]').map((c) => c.dataset['pdCell']);
  const key = (el: Element, k: string, init: KeyboardEventInit = {}) => el.dispatchEvent(new KeyboardEvent('keydown', { key: k, bubbles: true, cancelable: true, ...init }));
  const button = (text: string) => qa('button').find((b) => b.textContent?.trim() === text) as HTMLButtonElement;
  const ev = (type: string, x: number, y: number) => new PointerEvent(type, { bubbles: true, clientX: x, clientY: y, pointerId: 1, button: 0 });
  const edit = async () => {
    f.componentInstance.editing.set(true);
    await settle(f);
  };

  async function create(): Promise<void> {
    f = TestBed.createComponent(Host);
    document.body.appendChild(f.nativeElement);
    await settle(f);
  }

  beforeEach(() => {
    backend = new FakeBackend();
    sched = new ManualFrameScheduler();
    hostWidth = 1200;
    TestBed.configureTestingModule({
      providers: [provideZonelessChangeDetection(), { provide: PD_WIDGET_GRID_BACKEND, useValue: backend }, { provide: PD_FRAME_SCHEDULER, useValue: sched }],
    });
    registerPaddockIcons(TestBed.inject(MatIconRegistry), TestBed.inject(DomSanitizer));
    say = vi.spyOn(TestBed.inject(LiveAnnouncer), 'announce');
    vi.spyOn(Element.prototype, 'getBoundingClientRect').mockImplementation(function (this: Element) {
      if (this.classList.contains('pd-widget-grid')) return rect(0, 0, hostWidth, 600);
      if (this.classList.contains('pd-wg-grid')) return rect(0, 0, 1200, 600);
      return rect(0, 0, 100, 100);
    });
    HTMLElement.prototype.animate = undefined as never;
  });
  afterEach(() => {
    document.documentElement.removeAttribute('data-motion');
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
    f.nativeElement.remove();
  });

  describe('states', () => {
    it('shows a skeleton while there is no view, then the error with Retry, then the empty state', async () => {
      await create();
      f.componentInstance.view.set(null);
      await settle(f);
      expect(q('.pd-wg-skeleton')!.getAttribute('aria-busy')).toBe('true');
      expect(q('.pd-wg-skeleton')!.getAttribute('aria-label')).toBe('Loading the layout');
      expect(q('.pd-wg-bar button')).toBeNull();
      f.componentInstance.error.set('The agent did not answer.');
      await settle(f);
      expect(q('.pd-wg-state')!.getAttribute('role')).toBe('alert');
      expect(q('.pd-wg-state')!.textContent).toContain('The agent did not answer.');
      button('Retry').click();
      expect(f.componentInstance.retries).toBe(1);
      f.componentInstance.view.set(mkView({ layout: { version: 2, placements: [], presets: [] }, arrangements: [], moves: [] }));
      await settle(f);
      expect(q('.pd-wg-empty')!.textContent).toContain('Nothing on the dashboard yet');
      button('Add widget').click();
      expect(f.componentInstance.adds).toBe(1);
    });

    it('draws the widgets in reading order on their grid lines, with the app template and the frame chrome', async () => {
      await create();
      expect(ids()).toEqual(['a', 'b', 'c']);
      expect(cell('a').style.gridColumn).toBe('1 / span 6');
      expect(cell('b').style.gridColumn).toBe('7 / span 6');
      expect(cell('c').style.gridRow).toBe('5 / span 4');
      expect(q('.pd-wg-grid')!.style.gridTemplateColumns).toBe('repeat(12, minmax(0, 1fr))');
      expect(q('.pd-wg-grid')!.style.gridTemplateRows).toBe('repeat(8, var(--pd-widget-row))');
      expect(cell('a').textContent).toContain('cpu text');
      expect(cell('a').querySelector('.pd-w-title')!.textContent).toBe('CPU');
      expect(cell('a').querySelector('.pd-w-meta')!.textContent).toBe('3 volumes');
      expect(cell('a').querySelector('pd-widget-frame')!.classList.contains('pd-widget-frame--card')).toBe(true);
      expect(cell('a').getAttribute('role')).toBeNull();
      expect(cell('a').getAttribute('tabindex')).toBeNull();
      expect(qa('.pd-wg-h')).toEqual([]);
    });

    it('shows the notes once', async () => {
      await create();
      f.componentInstance.view.set(mkView({ notes: [{ code: 'damaged', message: 'The saved layout was damaged.' }] }));
      await settle(f);
      expect(q('.pd-wg-feedback')!.textContent).toContain('The saved layout was damaged.');
    });

    it('titles an unknown kind by its name and draws a placement the arrangement lacks stacked below, full width', async () => {
      await create();
      const v = mkView();
      const odd: View = { ...v, layout: { ...v.layout, placements: [...v.layout.placements, place('z', 'mystery', R(0, 99, 3, 0))] }, arrangements: v.arrangements.slice(0, 1) };
      f.componentInstance.view.set(odd);
      await settle(f);
      expect(ids()).toEqual(['a', 'b', 'c', 'z']);
      expect(cell('z').querySelector('.pd-w-title')!.textContent).toBe('mystery');
      expect(cell('z').style.gridColumn).toBe('1 / span 12');
      expect(cell('z').style.gridRow).toBe('9 / span 1');
    });

    it('falls back to one column when the view has no arrangements, and ignores an arrangement entry with no placement', async () => {
      await create();
      const v = mkView();
      f.componentInstance.view.set({ ...v, arrangements: [] });
      await settle(f);
      expect(ids()).toEqual(['a', 'b', 'c']);
      expect(cell('a').style.gridColumn).toBe('1 / span 1');
      expect(button('Edit layout').disabled).toBe(true);
      const first = v.arrangements[0]!;
      f.componentInstance.view.set({ ...v, arrangements: [{ ...first, placements: [...first.placements, { id: 'ghost', rect: R(0, 0, 1, 1) }] }] });
      await settle(f);
      expect(ids()).toEqual(['a', 'b', 'c']);
    });
  });

  describe('breakpoints', () => {
    it('picks the arrangement by container width', async () => {
      await create();
      f.componentInstance.width.set(700);
      await settle(f);
      expect(q('.pd-wg-grid')!.style.gridTemplateColumns).toBe('repeat(6, minmax(0, 1fr))');
      expect(cell('b').style.gridColumn).toBe('4 / span 3');
      f.componentInstance.width.set(100);
      await settle(f);
      expect(q('.pd-wg-grid')!.style.gridTemplateColumns).toBe('repeat(1, minmax(0, 1fr))');
      expect(cell('c').style.gridRow).toBe('9 / span 4');
      f.componentInstance.width.set(840);
      await settle(f);
      expect(cell('b').style.gridColumn).toBe('7 / span 6');
    });

    it('disables Edit layout below the first breakpoint and says why', async () => {
      await create();
      f.componentInstance.width.set(700);
      await settle(f);
      const b = button('Edit layout');
      expect(b.disabled).toBe(true);
      const reason = q('.pd-wg-reason')!;
      expect(reason.textContent).toBe('Arranging needs a wider dashboard. Make the window wider or collapse the sidebar.');
      expect(b.getAttribute('aria-describedby')).toBe(reason.id);
    });

    it('hides the bar when asked, and has none for an empty layout', async () => {
      await create();
      expect(button('Edit layout')).toBeDefined();
      f.componentInstance.view.set(mkView({ layout: { version: 2, placements: [], presets: [] }, arrangements: [], moves: [] }));
      await settle(f);
      expect(button('Edit layout')).toBeUndefined();
    });

    it('disables Edit layout for a read-only layout without the width reason', async () => {
      await create();
      f.componentInstance.view.set(mkView({ editable: false }));
      await settle(f);
      expect(button('Edit layout').disabled).toBe(true);
      expect(q('.pd-wg-reason')).toBeNull();
    });

    it('leaves edit mode when the container narrows below the first breakpoint', async () => {
      await create();
      await edit();
      expect(f.componentInstance.editing()).toBe(true);
      f.componentInstance.width.set(500);
      await settle(f);
      expect(f.componentInstance.editing()).toBe(false);
    });

    it('measures its own width when the app gives none, and follows a resize', async () => {
      let observed: ResizeObserverCallback = () => undefined;
      vi.stubGlobal('ResizeObserver', class { constructor(cb: ResizeObserverCallback) { observed = cb; } observe() { /* test double */ } disconnect() { /* test double */ } });
      hostWidth = 700;
      f = TestBed.createComponent(Host);
      f.componentInstance.width.set(null);
      document.body.appendChild(f.nativeElement);
      await settle(f);
      expect(q('.pd-wg-grid')!.style.gridTemplateColumns).toBe('repeat(6, minmax(0, 1fr))');
      observed([{ contentRect: { width: 1000 } } as ResizeObserverEntry], {} as ResizeObserver);
      await settle(f);
      expect(q('.pd-wg-grid')!.style.gridTemplateColumns).toBe('repeat(12, minmax(0, 1fr))');
      observed([], {} as ResizeObserver);
      await settle(f);
      expect(q('.pd-wg-grid')!.style.gridTemplateColumns).toBe('repeat(1, minmax(0, 1fr))');
    });

    it('works without a ResizeObserver', async () => {
      vi.stubGlobal('ResizeObserver', undefined);
      f = TestBed.createComponent(Host);
      f.componentInstance.width.set(null);
      document.body.appendChild(f.nativeElement);
      await settle(f);
      expect(ids()).toEqual(['a', 'b', 'c']);
    });
  });

  describe('edit mode', () => {
    it('opens with Edit layout and closes with Done; widgets become named groups with a size and a hint, bodies inert', async () => {
      await create();
      button('Edit layout').click();
      await settle(f);
      expect(f.componentInstance.editing()).toBe(true);
      expect(q('.pd-wg-bar-title')!.textContent).toBe('Editing layout');
      const a = cell('a');
      expect(a.getAttribute('role')).toBe('group');
      expect(a.getAttribute('aria-roledescription')).toBe('widget');
      expect(a.getAttribute('aria-label')).toBe('CPU');
      expect(a.getAttribute('tabindex')).toBe('0');
      const [sizeId, hintId] = a.getAttribute('aria-describedby')!.split(' ');
      expect(document.getElementById(sizeId!)!.textContent).toBe('6 by 4, position 1 of 3');
      expect(document.getElementById(hintId!)!.textContent).toContain('Drag to move.');
      expect(cell('c').getAttribute('aria-describedby')).toContain('-c');
      expect(document.getElementById(cell('c').getAttribute('aria-describedby')!.split(' ')[0]!)!.textContent).toBe('12 by 4, position 3 of 3');
      expect(a.querySelector('pd-widget-frame')!.hasAttribute('inert')).toBe(true);
      expect(a.querySelectorAll('.pd-wg-h')).toHaveLength(3);
      expect(button('Undo').disabled).toBe(true);
      expect(button('Redo').disabled).toBe(true);
      button('Add widget').click();
      expect(f.componentInstance.adds).toBe(1);
      button('Done').click();
      await settle(f);
      expect(f.componentInstance.editing()).toBe(false);
      expect(cell('a').querySelector('pd-widget-frame')!.hasAttribute('inert')).toBe(false);
    });
  });

  describe('keyboard', () => {
    beforeEach(async () => {
      await create();
      await edit();
      backend.edit.mockImplementation(() => Promise.resolve(moved()));
    });

    it('arrow keys nudge, with the revision the view was computed on; the answer is drawn, announced and focus returns to the widget', async () => {
      cell('a').focus();
      const e = key(cell('a'), 'ArrowRight');
      expect(e).toBe(false);
      expect(backend.edit).toHaveBeenCalledWith({ arrange: { gesture: { nudge: { id: 'a', dir: 'right' } } } }, 7);
      await settle(f);
      expect(ids()).toEqual(['b', 'a', 'c']);
      expect(f.componentInstance.changes).toHaveLength(1);
      expect(say).toHaveBeenCalledWith('CPU moved right, after Network. Position 2 of 3.', 'polite');
      expect(q('.pd-wg-feedback')!.textContent).toContain('CPU moved right, after Network. Position 2 of 3.');
      expect(document.activeElement).toBe(cell('a'));
      expect(button('Undo').disabled).toBe(false);
    });

    it('Shift with arrows grows, Home and End go first and last, Delete and Backspace remove', async () => {
      key(cell('a'), 'ArrowRight', { shiftKey: true });
      await settle(f);
      key(cell('a'), 'ArrowLeft', { shiftKey: true });
      await settle(f);
      key(cell('a'), 'ArrowDown', { shiftKey: true });
      await settle(f);
      key(cell('a'), 'ArrowUp', { shiftKey: true });
      await settle(f);
      key(cell('b'), 'Home');
      await settle(f);
      key(cell('b'), 'End');
      await settle(f);
      expect(backend.edit.mock.calls.map((c) => (c[0] as { arrange: { gesture: unknown } }).arrange.gesture)).toEqual([
        { grow: { id: 'a', dir: 'right' } },
        { grow: { id: 'a', dir: 'left' } },
        { grow: { id: 'a', dir: 'down' } },
        { grow: { id: 'a', dir: 'up' } },
        { toFirst: { id: 'b' } },
        { toLast: { id: 'b' } },
      ]);
      backend.edit.mockClear();
      key(cell('c'), 'Delete');
      await settle(f);
      key(cell('c'), 'Backspace');
      await settle(f);
      expect(backend.edit.mock.calls.map((c) => c[0])).toEqual([{ remove: { id: 'c' } }, { remove: { id: 'c' } }]);
      expect(say).toHaveBeenCalledWith('Disk removed. Undo is in the bar.', 'polite');
    });

    it('a key the engine says would change nothing is not sent and says why', async () => {
      key(cell('a'), 'ArrowLeft');
      await settle(f);
      expect(backend.edit).not.toHaveBeenCalled();
      expect(say).toHaveBeenCalledWith('CPU is already at the left edge.', 'polite');
      expect(q('.pd-wg-feedback--bad')!.getAttribute('role')).toBe('alert');
    });

    it('says the engine-style words for every refused key, and clears them on Done and on the next key', async () => {
      f.componentInstance.view.set(mkView({ moves: [{ id: 'a', moves: { ...ALL, left: false, first: false, wider: false, shorter: false } }] }));
      await settle(f);
      for (const [k, shift, text] of [['Home', false, 'CPU is already first.'], ['ArrowRight', true, "CPU can't get any larger."], ['ArrowUp', true, "CPU can't get any smaller."]] as const) {
        key(cell('a'), k, { shiftKey: shift });
        await settle(f);
        expect(q('.pd-wg-feedback')!.textContent).toContain(text);
      }
      f.componentInstance.view.set(mkView({ moves: [{ id: 'a', moves: { ...ALL, last: false } }] }));
      await settle(f);
      key(cell('a'), 'End');
      await settle(f);
      expect(q('.pd-wg-feedback')!.textContent).toContain('CPU is already last.');
      button('Done').click();
      await settle(f);
      expect(q('.pd-wg-feedback')).toBeNull();
    });

    it('ignores other keys, modified keys, and keys that come from inside the widget', async () => {
      key(cell('a'), 'x');
      key(cell('a'), 'ArrowRight', { altKey: true });
      key(cell('a'), 'ArrowRight', { ctrlKey: true });
      cell('a').querySelector('.pd-wg-more')!.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight', bubbles: true }));
      f.componentInstance.editing.set(false);
      await settle(f);
      key(cell('a'), 'ArrowRight');
      expect(backend.edit).not.toHaveBeenCalled();
    });

    it('Escape leaves edit mode when no gesture is active', async () => {
      key(cell('a'), 'Escape');
      await settle(f);
      expect(f.componentInstance.editing()).toBe(false);
    });

    it('Ctrl or Command Z undoes, with Shift redoes, and only in edit mode', async () => {
      key(cell('a'), 'z', { ctrlKey: true });
      await settle(f);
      key(cell('a'), 'Z', { metaKey: true, shiftKey: true });
      await settle(f);
      key(cell('a'), 'y', { ctrlKey: true });
      await settle(f);
      expect(backend.edit.mock.calls.map((c) => c[0])).toEqual(['undo', 'redo']);
      expect(say).toHaveBeenCalledWith('Undone.', 'polite');
      expect(say).toHaveBeenCalledWith('Redone.', 'polite');
      f.componentInstance.editing.set(false);
      await settle(f);
      key(cell('a'), 'z', { ctrlKey: true });
      expect(backend.edit).toHaveBeenCalledTimes(2);
    });

    it('Undo and Redo buttons follow the view', async () => {
      backend.edit.mockImplementation(() => Promise.resolve(mkView({ canUndo: true, canRedo: true }, 8)));
      f.componentInstance.view.set(mkView({ canUndo: true, canRedo: true }));
      await settle(f);
      button('Undo').click();
      await settle(f);
      button('Redo').click();
      await settle(f);
      expect(backend.edit.mock.calls.map((c) => c[0])).toEqual(['undo', 'redo']);
    });

    it('takes one edit at a time', async () => {
      let done: (v: View) => void = () => undefined;
      backend.edit.mockImplementation(() => new Promise<View>((r) => (done = r)));
      key(cell('a'), 'ArrowRight');
      key(cell('a'), 'ArrowDown');
      key(cell('a'), 'Delete');
      key(cell('a'), 'z', { ctrlKey: true });
      await settle(f);
      expect(backend.edit).toHaveBeenCalledTimes(1);
      expect(button('Undo').disabled).toBe(true);
      done(moved());
      await settle(f);
      key(cell('b'), 'ArrowDown');
      expect(backend.edit).toHaveBeenCalledTimes(2);
    });

    it('a resolved move that changed nothing says so, and a vanished widget is ignored', async () => {
      backend.edit.mockImplementation(() => Promise.resolve(mkView({}, 8)));
      key(cell('a'), 'ArrowDown');
      await settle(f);
      expect(say).toHaveBeenCalledWith('CPU stays where it is.', 'polite');
      backend.edit.mockImplementation(() => Promise.resolve({ ...mkView({}, 9), arrangements: [] }));
      key(cell('a'), 'ArrowDown');
      await settle(f);
      expect(say).toHaveBeenLastCalledWith('CPU stays where it is.', 'polite');
    });
  });

  describe('engine answers', () => {
    beforeEach(async () => {
      await create();
      await edit();
    });

    it('on stale, reloads and retries once with the fresh revision', async () => {
      backend.edit.mockRejectedValueOnce('stale').mockResolvedValueOnce(moved(12));
      backend.reload.mockResolvedValue(mkView({}, 11));
      key(cell('a'), 'ArrowRight');
      await settle(f);
      expect(backend.reload).toHaveBeenCalledTimes(1);
      expect(backend.edit.mock.calls.map((c) => c[1])).toEqual([7, 11]);
      expect(ids()).toEqual(['b', 'a', 'c']);
      expect(f.componentInstance.changes.map((v) => v.revision)).toEqual([11, 12]);
    });

    it('on a second stale, stops with a message', async () => {
      backend.edit.mockRejectedValue('stale');
      backend.reload.mockResolvedValue(mkView({}, 11));
      key(cell('a'), 'ArrowRight');
      await settle(f);
      expect(backend.edit).toHaveBeenCalledTimes(2);
      expect(say).toHaveBeenCalledWith('The layout changed elsewhere. Try again.', 'polite');
    });

    it('says what went wrong when the reload fails or the engine refuses', async () => {
      backend.edit.mockRejectedValueOnce('stale');
      backend.reload.mockRejectedValue(new Error('down'));
      key(cell('a'), 'ArrowRight');
      await settle(f);
      expect(say).toHaveBeenLastCalledWith("Couldn't change the layout. Try again.", 'polite');
      backend.edit.mockRejectedValue({ blocked: { edge: 'right' } });
      key(cell('b'), 'ArrowDown');
      await settle(f);
      expect(say).toHaveBeenLastCalledWith('Network is already at the right edge.', 'polite');
      expect(ids()).toEqual(['a', 'b', 'c']);
    });
  });

  describe('the menu', () => {
    const items = () => [...document.querySelectorAll<HTMLElement>('.cdk-overlay-container [role=menuitem]')];

    it('lists the ten gestures and Remove, disables what the engine says would change nothing, and runs the chosen one', async () => {
      await create();
      await edit();
      backend.edit.mockResolvedValue(moved());
      (cell('a').querySelector('.pd-wg-more') as HTMLElement).click();
      await settle(f);
      expect(items().map((i) => i.textContent?.trim())).toEqual(['Move left', 'Move right', 'Move up', 'Move down', 'Move CPU to first place', 'Move CPU to the end', 'Wider', 'Narrower', 'Taller', 'Shorter', 'Options…', 'Remove']);
      expect(items().map((i) => i.hasAttribute('disabled') || i.getAttribute('aria-disabled') === 'true')).toEqual([true, false, false, false, false, false, false, false, false, false, false, false]);
      items()[1]!.click();
      await settle(f);
      expect(backend.edit).toHaveBeenCalledWith({ arrange: { gesture: { nudge: { id: 'a', dir: 'right' } } } }, 7);
      (cell('c').querySelector('.pd-wg-more') as HTMLElement).click();
      await settle(f);
      items().at(-1)!.click();
      await settle(f);
      expect(backend.edit).toHaveBeenLastCalledWith({ remove: { id: 'c' } }, 8);
    });

    it('offers Options… for kinds that have them, and an undo toast after Remove that takes it back', async () => {
      await create();
      await edit();
      (cell('a').querySelector('.pd-wg-more') as HTMLElement).click();
      await settle(f);
      expect(items().map((i) => i.textContent?.trim())).toContain('Options…');
      items().find((i) => i.textContent?.trim() === 'Options…')!.click();
      expect(f.componentInstance.opened).toBe('a');
      await settle(f);
      (cell('b').querySelector('.pd-wg-more') as HTMLElement).click();
      await settle(f);
      expect(items().map((i) => i.textContent?.trim())).not.toContain('Options…');
      backend.edit.mockResolvedValue(mkView({ canUndo: true }, 8));
      items().at(-1)!.click();
      await settle(f);
      const snack = document.querySelector('.pd-undo-snack')!;
      expect(snack.textContent).toContain('Network removed.');
      backend.edit.mockClear();
      backend.edit.mockResolvedValue(mkView({}, 9));
      (snack.querySelector('button') as HTMLElement).click();
      await settle(f);
      expect(backend.edit).toHaveBeenCalledWith('undo', 8);
    });

    it('ignores a menu action for a widget that is gone', async () => {
      await create();
      await edit();
      const grid = f.debugElement.children[0]!.componentInstance as { removeById(id: string): void; runMove(id: string, m: string): Promise<void> };
      grid.removeById('nope');
      await grid.runMove('nope', 'left');
      expect(backend.edit).not.toHaveBeenCalled();
    });
  });

  describe('pointer gestures', () => {
    const down = (id: string, x: number, y: number, on: Element = cell(id)) => on.dispatchEvent(ev('pointerdown', x, y));
    const move = (id: string, x: number, y: number) => cell(id).dispatchEvent(ev('pointermove', x, y));
    const up = (id: string) => cell(id).dispatchEvent(ev('pointerup', 0, 0));
    const replyFor = (view: View, seq: number): PreviewReply<W, string, string> => ({ seq, view });

    beforeEach(async () => {
      await create();
      await edit();
    });

    it('previews each new cell with a counter, draws the placeholder, guides and previewed places, and drops with an arrange edit', async () => {
      backend.preview.mockImplementation((r) => Promise.resolve(replyFor(moved(7), r.seq)));
      backend.edit.mockResolvedValue(moved());
      down('a', 50, 50);
      move('a', 50 + 405, 50);
      sched.flush();
      await settle(f);
      expect(backend.preview).toHaveBeenCalledWith({ surface: 'dashboard', revision: 7, gesture: { place: { id: 'a', rect: R(4, 0, 6, 4) } }, seq: 1 });
      expect(cell('a').style.transform).toContain('translate(405px');
      expect(q('.pd-wg-ph')!.style.gridColumn).toBe('7 / span 6');
      expect(q('.pd-wg-guide--left')!.style.gridColumn).toBe('7 / span 1');
      expect(q('.pd-wg-guide--right')!.style.gridColumn).toBe('12 / span 6'.replace('12 / span 6', '12 / span 1'));
      expect(q('.pd-wg-guide--left')!.getAttribute('aria-hidden')).toBe('true');
      expect(q('.pd-wg-chip')).toBeNull();
      // The DOM keeps its order while a preview is drawn (moving the dragged node would drop its pointer capture).
      expect(ids()).toEqual(['a', 'b', 'c']);
      expect(cell('b').style.gridColumn).toBe('1 / span 6');
      expect(q('.pd-wg-grid')!.style.gridTemplateRows).toBe('repeat(16, var(--pd-widget-row))');
      up('a');
      await settle(f);
      expect(backend.edit).toHaveBeenCalledWith({ arrange: { gesture: { place: { id: 'a', rect: R(4, 0, 6, 4) } } } }, 7);
      expect(say).toHaveBeenCalledWith('CPU moved here, after Network. Position 2 of 3.', 'polite');
      expect(q('.pd-wg-ph')).toBeNull();
      expect(ids()).toEqual(['b', 'a', 'c']);
      expect(f.componentInstance.changes).toHaveLength(1);
    });

    it('drops replies that are not the latest', async () => {
      const pending: ((v: PreviewReply<W, string, string>) => void)[] = [];
      backend.preview.mockImplementation(() => new Promise((r) => pending.push(r)));
      down('a', 50, 50);
      move('a', 50 + 405, 50);
      sched.flush();
      move('a', 50 + 810, 50);
      sched.flush();
      expect(backend.preview.mock.calls.map((c) => c[0].seq)).toEqual([1, 2]);
      pending[0]!(replyFor(moved(), 1));
      await settle(f);
      expect(cell('a').style.gridColumn).toBe('1 / span 6');
      pending[1]!(replyFor(moved(), 2));
      await settle(f);
      expect(cell('a').style.gridColumn).toBe('7 / span 6');
    });

    it('a resize shows the size chip and the engine limit in words', async () => {
      backend.preview.mockImplementationOnce((r) => Promise.resolve(replyFor(mkView(), r.seq))).mockRejectedValueOnce({ blocked: 'largest' });
      down('a', 100, 100, cell('a').querySelector('.pd-wg-h--se')!);
      move('a', 100 + 695, 100 + 124);
      sched.flush();
      await settle(f);
      expect(q('.pd-wg-chip')!.textContent).toBe('8 × 6');
      move('a', 100 + 1000, 100);
      sched.flush();
      await settle(f);
      expect(q('.pd-wg-chip')!.textContent).toBe('Largest size');
      expect(say).toHaveBeenCalledWith("CPU can't get any larger.", 'polite');
    });

    it('says Smallest size, reloads on a stale preview, and ignores any other preview failure', async () => {
      backend.preview.mockRejectedValueOnce({ blocked: 'smallest' }).mockRejectedValueOnce('stale').mockRejectedValueOnce(new Error('x')).mockRejectedValueOnce({ blocked: 'unchanged' });
      backend.reload.mockResolvedValue(mkView({}, 9));
      down('a', 100, 100, cell('a').querySelector('.pd-wg-h--se')!);
      for (const dy of [0, 40, 80, 120]) {
        move('a', 100 - 400, 100 + dy);
        sched.flush();
        await settle(f);
      }
      expect(q('.pd-wg-chip')!.textContent).toBe('Smallest size');
      expect(backend.reload).toHaveBeenCalledTimes(1);
      expect(f.componentInstance.changes.map((v) => v.revision)).toEqual([9]);
    });

    it('says when the reload after a stale preview fails', async () => {
      backend.preview.mockRejectedValue('stale');
      backend.reload.mockRejectedValue(new Error('down'));
      down('a', 50, 50);
      move('a', 50 + 405, 50);
      sched.flush();
      await settle(f);
      expect(say).toHaveBeenCalledWith("Couldn't read the layout again. Try again.", 'polite');
    });

    it('Escape cancels: the stored layout is back and no edit is sent', async () => {
      backend.preview.mockImplementation((r) => Promise.resolve(replyFor(moved(7), r.seq)));
      down('a', 50, 50);
      move('a', 50 + 405, 50);
      sched.flush();
      await settle(f);
      key(cell('a'), 'Escape');
      document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }));
      await settle(f);
      expect(ids()).toEqual(['a', 'b', 'c']);
      expect(q('.pd-wg-ph')).toBeNull();
      expect(backend.edit).not.toHaveBeenCalled();
      expect(f.componentInstance.editing()).toBe(true);
    });

    it('a reply that arrives after the gesture ended is dropped', async () => {
      let late: (v: PreviewReply<W, string, string>) => void = () => undefined;
      backend.preview.mockImplementation(() => new Promise((r) => (late = r)));
      down('a', 50, 50);
      move('a', 50 + 405, 50);
      sched.flush();
      document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }));
      late(replyFor(moved(), 1));
      await settle(f);
      expect(ids()).toEqual(['a', 'b', 'c']);
    });

    it('a drop on the same cells sends nothing', async () => {
      backend.preview.mockImplementation((r) => Promise.resolve(replyFor(mkView(), r.seq)));
      down('a', 50, 50);
      move('a', 50 + 405, 50);
      sched.flush();
      move('a', 50, 50);
      sched.flush();
      await settle(f);
      up('a');
      await settle(f);
      expect(backend.edit).not.toHaveBeenCalled();
      expect(q('.pd-wg-ph')).toBeNull();
    });

    it('a refused drop says why and leaves the layout alone', async () => {
      backend.preview.mockImplementation((r) => Promise.resolve(replyFor(moved(7), r.seq)));
      backend.edit.mockRejectedValue({ blocked: 'unchanged' });
      down('a', 50, 50);
      move('a', 50 + 405, 50);
      sched.flush();
      await settle(f);
      up('a');
      await settle(f);
      expect(say).toHaveBeenCalledWith('CPU stays where it is.', 'polite');
      expect(ids()).toEqual(['a', 'b', 'c']);
    });

    it('does not start a gesture outside edit mode, on the menu button, or while an edit runs', async () => {
      f.componentInstance.editing.set(false);
      await settle(f);
      down('a', 50, 50);
      move('a', 450, 50);
      sched.flush();
      expect(backend.preview).not.toHaveBeenCalled();
      await edit();
      down('a', 50, 50, cell('a').querySelector('.pd-wg-more')!);
      move('a', 450, 50);
      sched.flush();
      expect(backend.preview).not.toHaveBeenCalled();
    });

    it('reads the grid geometry from the element (gap and row unit from CSS when present)', async () => {
      const real = getComputedStyle;
      vi.stubGlobal('getComputedStyle', (el: Element) => (el.classList.contains('pd-wg-grid') ? ({ columnGap: '12px', getPropertyValue: () => '24px' } as unknown as CSSStyleDeclaration) : real(el)));
      backend.preview.mockImplementation((r) => Promise.resolve(replyFor(mkView(), r.seq)));
      down('a', 50, 50);
      move('a', 50 + 320, 50 + 100);
      sched.flush();
      await settle(f);
      // column pitch (1200 - 11 * 12) / 12 + 12 = 100, row pitch 36
      expect(backend.preview).toHaveBeenCalledWith(expect.objectContaining({ gesture: { place: { id: 'a', rect: R(3, 3, 6, 4) } } }));
    });
  });

  describe('odd input and rare paths', () => {
    type Internals = { arrangementIndex(): number; rows(): unknown[]; columns(): number; allowed(id: string, m: string): boolean; undoRedo(w: string): Promise<void>; targetChanged(t: unknown): void; gestureStarted(t: unknown): void; dropped(t: unknown): Promise<void> };
    const grid = () => f.debugElement.children[0]!.componentInstance as unknown as Internals;

    it('never throws without a view: nothing to draw, nothing to send', async () => {
      await create();
      f.componentInstance.view.set(null);
      await settle(f);
      expect(grid().rows()).toEqual([]);
      expect(grid().columns()).toBe(1);
      expect(grid().arrangementIndex()).toBe(-1);
      expect(grid().allowed('a', 'left')).toBe(false);
      await grid().undoRedo('undo');
      grid().targetChanged({ id: 'a', kind: 'move', rect: R(0, 0, 1, 1) });
      expect(backend.edit).not.toHaveBeenCalled();
      expect(backend.preview).not.toHaveBeenCalled();
    });

    it('uses the last arrangement when none fits the width, and draws a view with no values yet', async () => {
      await create();
      const v = mkView();
      f.componentInstance.view.set({ ...v, arrangements: v.arrangements.map((a) => ({ ...a, minWidth: a.minWidth + 900 })) });
      f.componentInstance.values.set(null);
      f.componentInstance.width.set(100);
      await settle(f);
      expect(q('.pd-wg-grid')!.style.gridTemplateColumns).toBe('repeat(1, minmax(0, 1fr))');
      expect(cell('a').querySelector('section')!.getAttribute('aria-busy')).toBe('true');
    });

    it('draws the placeholder where the pointer is when the preview has no such widget, and names an unknown widget by its id', async () => {
      await create();
      await edit();
      grid().gestureStarted({ id: 'zz', kind: 'resize', rect: R(1, 1, 2, 2) });
      await settle(f);
      expect(q('.pd-wg-ph')!.style.gridColumn).toBe('2 / span 2');
      backend.preview.mockRejectedValue({ blocked: 'unchanged' });
      grid().targetChanged({ id: 'zz', kind: 'resize', rect: R(1, 1, 3, 3) });
      await settle(f);
      expect(say).toHaveBeenCalledWith('zz stays where it is.', 'polite');
    });

    it('a failed remove or undo says why and announces no success', async () => {
      await create();
      await edit();
      backend.edit.mockRejectedValue(new Error('down'));
      key(cell('a'), 'Delete');
      await settle(f);
      button('Undo').disabled = false;
      await grid().undoRedo('undo');
      expect(say).not.toHaveBeenCalledWith('Undone.', 'polite');
      expect(say).not.toHaveBeenCalledWith('CPU removed. Undo is in the bar.', 'polite');
      expect(say).toHaveBeenCalledWith("Couldn't change the layout. Try again.", 'polite');
    });

    it('ignores a late failure of a preview that a newer one replaced', async () => {
      await create();
      await edit();
      const rejects: ((e: unknown) => void)[] = [];
      backend.preview.mockImplementationOnce(() => new Promise((_, rej) => rejects.push(rej))).mockImplementationOnce((r) => Promise.resolve({ seq: r.seq, view: mkView() }));
      cell('a').dispatchEvent(new PointerEvent('pointerdown', { bubbles: true, clientX: 50, clientY: 50, pointerId: 1, button: 0 }));
      cell('a').dispatchEvent(new PointerEvent('pointermove', { bubbles: true, clientX: 455, clientY: 50, pointerId: 1 }));
      sched.flush();
      cell('a').dispatchEvent(new PointerEvent('pointermove', { bubbles: true, clientX: 860, clientY: 50, pointerId: 1 }));
      sched.flush();
      rejects[0]!({ blocked: 'largest' });
      await settle(f);
      expect(q('.pd-wg-chip')).toBeNull();
      expect(say).not.toHaveBeenCalledWith("CPU can't get any larger.", 'polite');
    });

    it('names a neighbour by its kind when the catalogue does not know it', async () => {
      await create();
      await edit();
      const v = moved();
      backend.edit.mockResolvedValue({ ...v, catalogue: { ...v.catalogue, kinds: v.catalogue.kinds.filter((k) => k.kind !== 'net') } });
      key(cell('a'), 'ArrowRight');
      await settle(f);
      expect(say).toHaveBeenCalledWith('CPU moved right, after net. Position 2 of 3.', 'polite');
    });

    it('passes a widget state action up', async () => {
      await create();
      f.componentInstance.values.set({ notice: null, placements: [{ id: 'a', state: { state: 'error', title: 'Failed', cause: null, next: { action: 'retry', label: 'Retry' } } }] });
      await settle(f);
      button('Retry').click();
      expect(f.componentInstance.actions).toEqual(['retry']);
    });
  });

  describe('motion', () => {
    it('glides moved widgets from their old places, and not in reduced motion', async () => {
      const animate = vi.fn();
      HTMLElement.prototype.animate = animate as never;
      let n = 0;
      vi.spyOn(Element.prototype, 'getBoundingClientRect').mockImplementation(function (this: Element) {
        return this.hasAttribute('data-pd-cell') ? rect(++n * 10, 0, 100, 100) : rect(0, 0, 1200, 600);
      });
      await create();
      await edit();
      backend.edit.mockResolvedValue(moved());
      key(cell('a'), 'ArrowRight');
      await settle(f);
      expect(animate).toHaveBeenCalled();
      expect(animate.mock.calls[0]![1]).toEqual({ duration: 160, easing: 'cubic-bezier(0.3, 0, 0.2, 1)' });
      animate.mockClear();
      document.documentElement.setAttribute('data-motion', 'reduce');
      backend.edit.mockResolvedValue(mkView({}, 9));
      key(cell('b'), 'ArrowLeft');
      await settle(f);
      expect(animate).not.toHaveBeenCalled();
    });

    it('does not animate where the browser has no Web Animations, or for a widget that did not move', async () => {
      await create();
      await edit();
      backend.edit.mockResolvedValue(moved());
      key(cell('a'), 'ArrowRight');
      await settle(f);
      HTMLElement.prototype.animate = vi.fn() as never;
      backend.edit.mockResolvedValue(moved(9));
      key(cell('a'), 'ArrowRight');
      await settle(f);
      expect(HTMLElement.prototype.animate).not.toHaveBeenCalled();
    });
  });
});
