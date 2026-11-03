import { ChangeDetectionStrategy, Component, provideZonelessChangeDetection, signal } from '@angular/core';
import { TestBed, type ComponentFixture } from '@angular/core/testing';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { Outcome, Rect } from '../generated/widgets';
import { ManualFrameScheduler, PD_FRAME_SCHEDULER } from './frame-scheduler';
import { PdGridGesture, reducedMotion, type GridGestureTarget } from './grid-gesture';
import { columnLines, moveTarget, resizeTarget, rowLines, sameRect, type GridGeometry } from './grid-math';
import { blockOf, blockText, cantText, errorText, howOf, isStale, outcomeText, MOVE_ACTIONS, MOVE_NAMES, moveOfKey, sizeText } from './grid-text';

const G: GridGeometry = { left: 100, top: 50, colWidth: 84, rowHeight: 24, gap: 16, columns: 12 };

describe('grid math', () => {
  it('rounds a dragged corner to the nearest cell and keeps the widget inside the grid', () => {
    expect(moveTarget(G, 0, 0, 4, 3)).toEqual({ x: 0, y: 0, w: 4, h: 3 });
    expect(moveTarget(G, 205, 85, 4, 3)).toEqual({ x: 2, y: 2, w: 4, h: 3 });
    expect(moveTarget(G, -300, -300, 4, 3)).toEqual({ x: 0, y: 0, w: 4, h: 3 });
    expect(moveTarget(G, 5000, 40, 4, 3)).toEqual({ x: 8, y: 1, w: 4, h: 3 });
    expect(moveTarget(G, 5000, 40, 20, 3).x).toBe(0);
  });

  it('rounds a resize to whole cells, at least 1 by 1 and inside the grid', () => {
    expect(resizeTarget(G, 2, 1, 84, 24)).toEqual({ x: 2, y: 1, w: 1, h: 1 });
    expect(resizeTarget(G, 2, 1, 4 * 84 + 3 * 16, 6 * 24 + 5 * 16)).toEqual({ x: 2, y: 1, w: 4, h: 6 });
    expect(resizeTarget(G, 2, 1, -50, -50)).toEqual({ x: 2, y: 1, w: 1, h: 1 });
    expect(resizeTarget(G, 10, 0, 5000, 24).w).toBe(2);
    expect(resizeTarget({ ...G, columns: 1 }, 0, 0, 5000, 24).w).toBe(1);
  });

  it('writes CSS grid lines and compares rectangles', () => {
    const r: Rect = { x: 2, y: 3, w: 4, h: 5 };
    expect(columnLines(r)).toBe('3 / span 4');
    expect(rowLines(r)).toBe('4 / span 5');
    expect(sameRect(r, { ...r })).toBe(true);
    for (const k of ['x', 'y', 'w', 'h'] as const) expect(sameRect(r, { ...r, [k]: 9 })).toBe(false);
  });
});

describe('grid words', () => {
  it('maps keys to the engine moves', () => {
    expect(moveOfKey('ArrowLeft', false)).toBe('left');
    expect(moveOfKey('ArrowRight', false)).toBe('right');
    expect(moveOfKey('ArrowUp', false)).toBe('up');
    expect(moveOfKey('ArrowDown', false)).toBe('down');
    expect(moveOfKey('ArrowLeft', true)).toBe('narrower');
    expect(moveOfKey('ArrowRight', true)).toBe('wider');
    expect(moveOfKey('ArrowUp', true)).toBe('shorter');
    expect(moveOfKey('ArrowDown', true)).toBe('taller');
    expect(moveOfKey('Home', false)).toBe('first');
    expect(moveOfKey('End', false)).toBe('last');
    expect(moveOfKey('Home', true)).toBeNull();
    expect(moveOfKey('End', true)).toBeNull();
    expect(moveOfKey('a', false)).toBeNull();
  });

  it('builds the gesture each move sends, in menu order', () => {
    expect(MOVE_NAMES).toEqual(['left', 'right', 'up', 'down', 'first', 'last', 'wider', 'narrower', 'taller', 'shorter']);
    expect(MOVE_ACTIONS.left.gesture('a')).toEqual({ nudge: { id: 'a', dir: 'left' } });
    expect(MOVE_ACTIONS.wider.gesture('a')).toEqual({ grow: { id: 'a', dir: 'right' } });
    expect(MOVE_ACTIONS.narrower.gesture('a')).toEqual({ grow: { id: 'a', dir: 'left' } });
    expect(MOVE_ACTIONS.taller.gesture('a')).toEqual({ grow: { id: 'a', dir: 'down' } });
    expect(MOVE_ACTIONS.shorter.gesture('a')).toEqual({ grow: { id: 'a', dir: 'up' } });
    expect(MOVE_ACTIONS.first.gesture('a')).toEqual({ toFirst: { id: 'a' } });
    expect(MOVE_ACTIONS.last.gesture('a')).toEqual({ toLast: { id: 'a' } });
  });

  it('says why a gesture was refused', () => {
    expect(blockText('CPU', { edge: 'left' })).toBe('CPU is already at the left edge.');
    expect(blockText('CPU', { edge: 'right' })).toBe('CPU is already at the right edge.');
    expect(blockText('CPU', { edge: 'up' })).toBe('CPU is already at the top edge.');
    expect(blockText('CPU', { edge: 'down' })).toBe('CPU is already at the bottom edge.');
    expect(blockText('CPU', 'largest')).toBe("CPU can't get any larger.");
    expect(blockText('CPU', 'smallest')).toBe("CPU can't get any smaller.");
    expect(blockText('CPU', 'unchanged')).toBe('CPU stays where it is.');
  });

  it('says why a key does nothing, in the words of a refusal', () => {
    expect(cantText('CPU', 'left')).toBe('CPU is already at the left edge.');
    expect(cantText('CPU', 'down')).toBe('CPU is already at the bottom edge.');
    expect(cantText('CPU', 'first')).toBe('CPU is already first.');
    expect(cantText('CPU', 'last')).toBe('CPU is already last.');
    expect(cantText('CPU', 'wider')).toBe("CPU can't get any larger.");
    expect(cantText('CPU', 'shorter')).toBe("CPU can't get any smaller.");
  });

  it('reads the engine rejections', () => {
    expect(isStale('stale')).toBe(true);
    expect(isStale({ blocked: 'largest' })).toBe(false);
    expect(blockOf({ blocked: 'smallest' })).toBe('smallest');
    expect(blockOf('stale')).toBeNull();
    expect(blockOf(null)).toBeNull();
    expect(errorText({ blocked: 'largest' }, 'CPU')).toBe("CPU can't get any larger.");
    expect(errorText({ sizeNotAllowed: {} }, 'CPU')).toBe("CPU can't be that size.");
    expect(errorText({ invalid: 'Bad id.' }, 'CPU')).toBe('Bad id.');
    expect(errorText({ unknownPlacement: 'x' }, 'CPU')).toBe("Couldn't change the layout. Try again.");
    expect(errorText(new Error('boom'), 'CPU')).toBe("Couldn't change the layout. Try again.");
    expect(errorText('nothingToUndo', 'CPU')).toBe("Couldn't change the layout. Try again.");
  });

  it('announces a move, a resize, both, a limit and nothing, from the engine outcome', () => {
    const a: Rect = { x: 0, y: 0, w: 4, h: 4 };
    const o = (over: Partial<Outcome> = {}): Outcome => ({ id: 'a', position: 6, of: 15, after: 'c', before: null, w: 4, h: 4, limit: null, ...over });
    expect(sizeText(a, 5, 15)).toBe('4 by 4, position 5 of 15');
    expect(outcomeText('Memory', o(), 'CPU', { nudge: { id: 'a', dir: 'right' } }, a)).toBe('Memory moved right, after CPU. Position 6 of 15.');
    expect(outcomeText('Memory', o({ position: 1, after: null }), null, { toFirst: { id: 'a' } }, a)).toBe('Memory moved to first, first. Position 1 of 15.');
    expect(outcomeText('Memory', o({ h: 6 }), null, { grow: { id: 'a', dir: 'down' } }, a)).toBe('Memory resized to 4 by 6.');
    expect(outcomeText('Memory', o({ h: 14, limit: 'largest' }), null, { grow: { id: 'a', dir: 'down' } }, a)).toBe("Memory resized to 4 by 14. Memory can't get any larger.");
    expect(outcomeText('Memory', o({ w: 3, position: 2 }), 'CPU', { place: { id: 'a', rect: a } }, a)).toBe('Memory moved here, after CPU. Position 2 of 15. Memory resized to 3 by 4.');
    expect(outcomeText('Memory', o(), 'CPU', { place: { id: 'a', rect: a } }, undefined)).toBe('Memory moved here, after CPU. Position 6 of 15.');
    expect(howOf({ toLast: { id: 'a' } })).toBe('to last');
    expect(howOf({ nudge: { id: 'a', dir: 'up' } })).toBe('up');
    expect(howOf({ toFirst: { id: 'a' } })).toBe('to first');
    expect(howOf({ place: { id: 'a', rect: a } })).toBe('here');
  });
});

describe('PD_FRAME_SCHEDULER', () => {
  it('steps by hand in a spec, running a frame at a time and forgetting what was cancelled', () => {
    const s = new ManualFrameScheduler();
    const order: string[] = [];
    s.request(() => { order.push('a'); s.request(() => order.push('c')); });
    const h = s.request(() => order.push('x'));
    s.cancel(h);
    s.request(() => order.push('b'));
    s.flush();
    expect(order).toEqual(['a', 'b']);
    s.flush();
    expect(order).toEqual(['a', 'b', 'c']);
  });


  it('runs on the next animation frame unless cancelled', () => {
    const run = vi.fn();
    let cb: FrameRequestCallback = () => undefined;
    const raf = vi.fn((f: FrameRequestCallback) => ((cb = f), 7));
    const caf = vi.fn();
    vi.stubGlobal('requestAnimationFrame', raf);
    vi.stubGlobal('cancelAnimationFrame', caf);
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection()] });
    const s = TestBed.inject(PD_FRAME_SCHEDULER);
    expect(s.request(run)).toBe(7);
    cb(0);
    expect(run).toHaveBeenCalledTimes(1);
    s.cancel(7);
    expect(caf).toHaveBeenCalledWith(7);
    vi.unstubAllGlobals();
  });
});

@Component({
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [PdGridGesture],
  template: `
    <div class="scroller"><div class="cell" tabindex="0" [pdGridGesture]="'a'" [pdGridRect]="rect()" [pdGridGeometry]="geometry" [pdGridDisabled]="disabled()" [pdGridScroller]="scroller()"
      (gestureStart)="log.push(['start', $event])" (targetChange)="log.push(['target', $event])" (dropped)="log.push(['drop', $event])" (cancelled)="log.push(['cancel'])">
      <span class="body">body</span><button class="btn" data-pd-nodrag>x</button><span class="he" data-pd-handle="e"></span><span class="hs" data-pd-handle="s"></span><span class="hse" data-pd-handle="se"></span><span class="hx" data-pd-handle="nw"></span>
    </div></div>
  `,
})
class GestureHost {
  readonly rect = signal<Rect>({ x: 0, y: 0, w: 2, h: 2 });
  readonly disabled = signal(false);
  readonly scroller = signal<HTMLElement | null>(null);
  g: GridGeometry = G;
  readonly geometry = (): GridGeometry => this.g;
  log: (readonly [string] | readonly [string, GridGestureTarget])[] = [];
}

describe('pdGridGesture', () => {
  let sched: ManualFrameScheduler;
  let f: ComponentFixture<GestureHost>;
  let cell: HTMLElement;

  const ev = (type: string, x: number, y: number, extra: PointerEventInit = {}) => new PointerEvent(type, { bubbles: true, clientX: x, clientY: y, pointerId: 1, button: 0, ...extra });
  const down = (x = 200, y = 100, on: Element = cell) => on.dispatchEvent(ev('pointerdown', x, y));
  const move = (x: number, y: number, extra: PointerEventInit = {}) => cell.dispatchEvent(ev('pointermove', x, y, extra));
  const up = () => cell.dispatchEvent(ev('pointerup', 0, 0));
  const names = () => f.componentInstance.log.map((l) => l[0]);

  beforeEach(() => {
    sched = new ManualFrameScheduler();
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection(), { provide: PD_FRAME_SCHEDULER, useValue: sched }] });
    vi.spyOn(Element.prototype, 'getBoundingClientRect').mockImplementation(() => ({ left: 100, top: 50, width: 184, height: 64, right: 284, bottom: 114, x: 100, y: 50, toJSON: () => ({}) }));
    f = TestBed.createComponent(GestureHost);
    document.body.appendChild(f.nativeElement);
    f.detectChanges();
    cell = f.nativeElement.querySelector('.cell');
  });
  afterEach(() => {
    document.documentElement.removeAttribute('data-motion');
    vi.restoreAllMocks();
    f.nativeElement.remove();
  });

  it('leaves a click alone: no travel past 4 px, no gesture', () => {
    down();
    move(202, 101);
    sched.flush();
    up();
    expect(names()).toEqual([]);
    expect(cell.style.transform).toBe('');
  });

  it('moves: lifts after 4 px, follows the pointer once per frame, reports each new cell and the drop', () => {
    down(200, 100);
    move(210, 100);
    move(420, 100);
    expect(names()).toEqual(['start']);
    expect(cell.classList.contains('pd-wg-lifted')).toBe(true);
    expect(sched.queue.size).toBe(1);
    sched.flush();
    expect(cell.style.transform).toBe('translate(220px, 0px) scale(1.02)');
    expect(f.componentInstance.log[1]).toEqual(['target', { id: 'a', kind: 'move', rect: { x: 2, y: 0, w: 2, h: 2 } }]);
    move(430, 100);
    sched.flush();
    expect(names()).toEqual(['start', 'target']);
    up();
    expect(f.componentInstance.log[2]).toEqual(['drop', { id: 'a', kind: 'move', rect: { x: 2, y: 0, w: 2, h: 2 } }]);
    expect(cell.style.transform).toBe('');
    expect(cell.classList.contains('pd-wg-lifted')).toBe(false);
  });

  it('follows the pointer without the lift scale in reduced motion', () => {
    document.documentElement.setAttribute('data-motion', 'reduce');
    down(200, 100);
    move(300, 100);
    sched.flush();
    expect(cell.style.transform).toBe('translate(100px, 0px)');
    expect(reducedMotion()).toBe(true);
    document.documentElement.setAttribute('data-motion', 'full');
    expect(reducedMotion()).toBe(false);
    document.documentElement.removeAttribute('data-motion');
    vi.stubGlobal('matchMedia', () => ({ matches: true }));
    expect(reducedMotion()).toBe(true);
    vi.stubGlobal('matchMedia', undefined);
    expect(reducedMotion()).toBe(false);
    vi.unstubAllGlobals();
  });

  it('resizes from the corner, the right edge and the bottom edge, with the size following the pointer', () => {
    const he = cell.querySelector('.he')!;
    down(284, 114, cell.querySelector('.hse')!);
    move(284 + 2 * 100, 114 + 80);
    sched.flush();
    expect(cell.style.width).toBe('384px');
    expect(cell.style.height).toBe('144px');
    expect(cell.classList.contains('pd-wg-resizing')).toBe(true);
    expect(f.componentInstance.log[1]).toEqual(['target', { id: 'a', kind: 'resize', rect: { x: 0, y: 0, w: 4, h: 4 } }]);
    up();
    expect(cell.style.width).toBe('');
    f.componentInstance.log = [];
    down(284, 114, he);
    move(484, 214);
    sched.flush();
    expect(cell.style.width).toBe('384px');
    expect(cell.style.height).toBe('64px');
    up();
    down(284, 114, cell.querySelector('.hs')!);
    move(484, 214);
    sched.flush();
    expect(cell.style.width).toBe('184px');
    expect(cell.style.height).toBe('164px');
    up();
    // An unknown handle name is a move.
    down(200, 100, cell.querySelector('.hx')!);
    move(300, 100);
    sched.flush();
    expect(cell.classList.contains('pd-wg-lifted')).toBe(true);
  });

  it('never lets a resize reach past the grid edge', () => {
    down(284, 114, cell.querySelector('.hse')!);
    move(5000, 114);
    sched.flush();
    // The grid ends at 100 + 12 * 100 - 16 = 1284; the cell starts at 100.
    expect(cell.style.width).toBe('1184px');
    up();
  });

  it('cancels with Escape, a cancelled pointer or a lost capture, and says so only when a gesture was active', () => {
    down();
    move(300, 100);
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }));
    expect(names()).toEqual(['start', 'cancel']);
    expect(cell.style.transform).toBe('');
    f.componentInstance.log = [];
    down();
    cell.dispatchEvent(ev('pointercancel', 0, 0));
    expect(names()).toEqual([]);
    down();
    move(300, 100);
    cell.dispatchEvent(ev('lostpointercapture', 0, 0));
    expect(names()).toEqual(['start', 'cancel']);
    f.componentInstance.log = [];
    down();
    move(300, 100);
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'a' }));
    expect(names()).toEqual(['start']);
  });

  it('does nothing after the session ended, for another pointer, a disabled cell, a button or a secondary button', () => {
    up();
    cell.dispatchEvent(ev('pointercancel', 0, 0));
    down();
    cell.dispatchEvent(ev('pointerdown', 5, 5));
    move(900, 900, { pointerId: 2 });
    sched.flush();
    expect(names()).toEqual([]);
    up();
    down(200, 100, cell.querySelector('.btn')!);
    move(300, 100);
    expect(names()).toEqual([]);
    cell.dispatchEvent(ev('pointerdown', 200, 100, { button: 2 }));
    move(300, 100);
    expect(names()).toEqual([]);
    f.componentInstance.disabled.set(true);
    f.detectChanges();
    down();
    move(300, 100);
    expect(names()).toEqual([]);
  });

  it('lets go of the pointer capture it holds', () => {
    const release = vi.fn();
    cell.hasPointerCapture = () => true;
    cell.releasePointerCapture = release;
    down();
    up();
    expect(release).toHaveBeenCalledWith(1);
  });

  it('focuses the cell on pointerdown and drops a pending frame when the gesture ends', () => {
    f.nativeElement.querySelector('.btn').focus();
    down();
    expect(document.activeElement).toBe(cell);
    move(300, 100);
    expect(sched.queue.size).toBe(1);
    up();
    expect(sched.queue.size).toBe(0);
  });

  it('scrolls the page near the bottom and top edges, a frame at a time', () => {
    let top = 500;
    const page = document.createElement('div');
    Object.defineProperty(page, 'scrollTop', { configurable: true, get: () => top, set: (v: number) => (top = v) });
    Object.defineProperty(document, 'scrollingElement', { configurable: true, value: page });
    vi.stubGlobal('innerHeight', 800);
    down(200, 700);
    move(200, 800);
    sched.flush();
    expect(top).toBe(516);
    expect(sched.queue.size).toBe(1);
    move(200, 400);
    sched.flush();
    expect(top).toBe(516);
    expect(sched.queue.size).toBe(0);
    move(200, 24);
    sched.flush();
    expect(top).toBe(508);
    up();
    vi.unstubAllGlobals();
    delete (document as unknown as Record<string, unknown>)['scrollingElement'];
  });

  it('scrolls its own scroller, and stops when it cannot move', () => {
    const el = f.nativeElement.querySelector('.scroller') as HTMLElement;
    let top = 100;
    let frozen = true;
    Object.defineProperty(el, 'scrollTop', { configurable: true, get: () => top, set: (v: number) => (top = frozen ? top : v) });
    f.componentInstance.scroller.set(el);
    f.detectChanges();
    down(200, 100);
    move(200, 110);
    sched.flush();
    expect(top).toBe(100);
    expect(sched.queue.size).toBe(0);
    frozen = false;
    move(200, 120);
    sched.flush();
    expect(top).toBe(116);
    up();
  });

  it('reads the grid again every frame, so a scroll moves the cell with the grid', () => {
    down(200, 100);
    move(200, 130);
    f.componentInstance.g = { ...G, top: 0 };
    sched.flush();
    expect(cell.style.transform).toBe('translate(0px, 80px) scale(1.02)');
  });

  it('puts the cell back when destroyed mid-gesture', () => {
    down();
    move(300, 100);
    sched.flush();
    f.destroy();
    expect(cell.style.transform).toBe('');
  });
});
