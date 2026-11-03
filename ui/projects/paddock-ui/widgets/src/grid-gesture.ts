import { DestroyRef, Directive, ElementRef, inject, input, output, Renderer2 } from '@angular/core';
import type { Rect } from '../generated/widgets';
import { PD_FRAME_SCHEDULER } from './frame-scheduler';
import { moveTarget, resizeTarget, sameRect, type GridGeometry } from './grid-math';

/** Pointer travel before a move starts, so a click on the frame stays a click. */
const MOVE_THRESHOLD = 4;
/** Distance from the scroll container's edge where auto-scroll starts, and its top speed in px per frame. */
const EDGE = 48;
const MAX_SPEED = 16;

/** What a gesture is doing: moving the frame, or resizing it from an edge or the corner. */
export type GridGestureKind = 'move' | 'resize';
type Handle = 'e' | 's' | 'se';

/** The snapped rectangle a gesture would end on. */
export interface GridGestureTarget {
  /** The placement. */
  readonly id: string;
  /** Move or resize. */
  readonly kind: GridGestureKind;
  /** The target cells. */
  readonly rect: Rect;
}

interface Session {
  readonly pointerId: number;
  readonly handle: Handle | null;
  readonly px0: number;
  readonly py0: number;
  px: number;
  py: number;
  readonly start: DOMRect;
  readonly g0: GridGeometry;
  readonly from: Rect;
  active: boolean;
  last: Rect;
  frame: number | null;
  readonly unlisten: (() => void)[];
}

/**
 * Pointer move and resize of one grid cell (ADR 0044 §5; not CDK drag-drop: D1). Pointerdown on the cell starts a move after
 * 4 px of travel; pointerdown on an element marked `data-pd-handle="e|s|se"` starts a resize. The cell follows the pointer
 * through one `transform` (or width and height) per animation frame, set with the renderer, never through change detection.
 * Each time the nearest cell changes `targetChange` fires (the grid asks the engine for a preview); release fires `dropped`;
 * Escape, a cancelled pointer or a lost capture fire `cancelled` and put the cell back. Near the scroll container's top or
 * bottom edge the page scrolls, a frame at a time. Elements marked `data-pd-nodrag` never start a gesture.
 */
@Directive({
  selector: '[pdGridGesture]',
  host: { '(pointerdown)': 'down($event)' },
})
export class PdGridGesture {
  private readonly host = inject<ElementRef<HTMLElement>>(ElementRef).nativeElement;
  private readonly renderer = inject(Renderer2);
  private readonly scheduler = inject(PD_FRAME_SCHEDULER);
  private session: Session | null = null;

  /** The placement's id. */
  readonly pdGridGesture = input.required<string>();
  /** The placement's current cells. */
  readonly pdGridRect = input.required<Rect>();
  /** Reads the grid's measures now (it is read again every frame, because the page may scroll). */
  readonly pdGridGeometry = input.required<() => GridGeometry>();
  /** True while gestures are off (not in edit mode). */
  readonly pdGridDisabled = input(false);
  /** The element that scrolls; the page when null. */
  readonly pdGridScroller = input<HTMLElement | null>(null);

  /** A gesture became active (past the threshold). */
  readonly gestureStart = output<GridGestureTarget>();
  /** The nearest cells changed. */
  readonly targetChange = output<GridGestureTarget>();
  /** The pointer was released after an active gesture. */
  readonly dropped = output<GridGestureTarget>();
  /** The gesture was cancelled; nothing is sent. */
  readonly cancelled = output<void>();

  constructor() {
    inject(DestroyRef).onDestroy(() => this.end());
  }

  protected down(e: PointerEvent): void {
    if (this.session || this.pdGridDisabled() || e.button !== 0) return;
    const target = e.target as Element;
    if (target.closest('[data-pd-nodrag]')) return;
    const h = target.closest('[data-pd-handle]')?.getAttribute('data-pd-handle');
    const handle: Handle | null = h === 'e' || h === 's' || h === 'se' ? h : null;
    const rect = this.pdGridRect();
    const session: Session = {
      pointerId: e.pointerId,
      handle,
      px0: e.clientX,
      py0: e.clientY,
      px: e.clientX,
      py: e.clientY,
      start: this.host.getBoundingClientRect(),
      g0: this.pdGridGeometry()(),
      from: rect,
      active: false,
      last: rect,
      frame: null,
      unlisten: [],
    };
    this.session = session;
    this.host.focus({ preventScroll: true });
    this.host.setPointerCapture(e.pointerId);
    const on = (event: string, fn: (ev: Event) => void) => session.unlisten.push(this.renderer.listen(this.host, event, fn));
    on('pointermove', (ev) => this.move(ev as PointerEvent));
    on('pointerup', () => this.up());
    on('pointercancel', () => this.cancel());
    on('lostpointercapture', () => this.cancel());
    session.unlisten.push(this.renderer.listen('document', 'keydown', (ev: KeyboardEvent) => ev.key === 'Escape' && this.cancel()));
  }

  private move(e: PointerEvent): void {
    const s = this.session;
    if (!s || e.pointerId !== s.pointerId) return;
    s.px = e.clientX;
    s.py = e.clientY;
    if (!s.active) {
      if (s.handle === null && Math.hypot(s.px - s.px0, s.py - s.py0) < MOVE_THRESHOLD) return;
      s.active = true;
      this.renderer.addClass(this.host, s.handle === null ? 'pd-wg-lifted' : 'pd-wg-resizing');
      this.gestureStart.emit(this.targetOf(s, s.from));
    }
    this.schedule();
  }

  private up(): void {
    const s = this.session as Session;
    const done = s.active ? this.targetOf(s, s.last) : null;
    this.end();
    if (done) this.dropped.emit(done);
  }

  private cancel(): void {
    const s = this.session as Session;
    const was = s.active;
    this.end();
    if (was) this.cancelled.emit();
  }

  private schedule(): void {
    const s = this.session as Session;
    if (s.frame === null) s.frame = this.scheduler.request(() => this.tick());
  }

  private tick(): void {
    const s = this.session as Session;
    s.frame = null;
    const scrolled = this.autoScroll(s);
    const g = this.pdGridGeometry()();
    const dx = s.px - s.px0;
    const dy = s.py - s.py0;
    const ox = g.left - s.g0.left;
    const oy = g.top - s.g0.top;
    let rect: Rect;
    if (s.handle === null) {
      const lift = reducedMotion() ? '' : ' scale(1.02)';
      this.renderer.setStyle(this.host, 'transform', `translate(${dx - ox}px, ${dy - oy}px)${lift}`);
      rect = moveTarget(g, s.start.left + dx - g.left, s.start.top + dy - g.top, s.from.w, s.from.h);
    } else {
      // Never wider than the grid's right edge, whatever the pointer does.
      const room = g.left + g.columns * (g.colWidth + g.gap) - g.gap - s.start.left - ox;
      const width = s.handle === 's' ? s.start.width : Math.min(room, s.start.width + dx - ox);
      const height = s.handle === 'e' ? s.start.height : s.start.height + dy - oy;
      this.renderer.setStyle(this.host, 'width', `${width}px`);
      this.renderer.setStyle(this.host, 'height', `${height}px`);
      rect = resizeTarget(g, s.from.x, s.from.y, width, height);
    }
    if (!sameRect(rect, s.last)) {
      s.last = rect;
      this.targetChange.emit(this.targetOf(s, rect));
    }
    if (scrolled) this.schedule();
  }

  /** Scrolls the container a little when the pointer is within 48 px of its top or bottom; true when it moved. */
  private autoScroll(s: Session): boolean {
    const scroller = this.pdGridScroller();
    const el = scroller ?? (document.scrollingElement as HTMLElement);
    const box = scroller ? scroller.getBoundingClientRect() : { top: 0, bottom: window.innerHeight };
    let speed = 0;
    if (s.py < box.top + EDGE) speed = -MAX_SPEED * Math.min(1, (box.top + EDGE - s.py) / EDGE);
    else if (s.py > box.bottom - EDGE) speed = MAX_SPEED * Math.min(1, (s.py - (box.bottom - EDGE)) / EDGE);
    if (speed === 0) return false;
    const before = el.scrollTop;
    el.scrollTop = before + speed;
    return el.scrollTop !== before;
  }

  private targetOf(s: Session, rect: Rect): GridGestureTarget {
    return { id: this.pdGridGesture(), kind: s.handle === null ? 'move' : 'resize', rect };
  }

  /** Stops listening, cancels the pending frame and puts the cell back. */
  private end(): void {
    const s = this.session;
    if (!s) return;
    this.session = null;
    s.unlisten.forEach((f) => f());
    if (s.frame !== null) this.scheduler.cancel(s.frame);
    for (const p of ['transform', 'width', 'height']) this.renderer.removeStyle(this.host, p);
    this.renderer.removeClass(this.host, 'pd-wg-lifted');
    this.renderer.removeClass(this.host, 'pd-wg-resizing');
    if (this.host.hasPointerCapture(s.pointerId)) this.host.releasePointerCapture(s.pointerId);
  }
}

/** True when the person (or the app's `data-motion`) wants no animation. */
export function reducedMotion(): boolean {
  const motion = document.documentElement.getAttribute('data-motion');
  if (motion === 'reduce') return true;
  if (motion === 'full') return false;
  return typeof matchMedia === 'function' && matchMedia('(prefers-reduced-motion: reduce)').matches;
}
