import { DOCUMENT } from '@angular/common';
import { Directive, ElementRef, inject, input, output, signal } from '@angular/core';

/** The room a pane may be resized within, px. */
export interface SashLimits {
  min: number;
  max: number;
}

/** Arrow key step, px. */
export const SASH_STEP = 16;
/** Arrow key step with Shift held, px. */
export const SASH_BIG_STEP = 64;

/** Clamps `width` to `limits` (`max` wins over `min` only if the limits are inverted, so the result is always inside them). */
export function clampSash(width: number, limits: SashLimits): number {
  return Math.round(Math.max(limits.min, Math.min(Math.max(limits.min, limits.max), width)));
}

/**
 * A vertical splitter on the inner edge of a pane. Put it on the element that is the grab area:
 * `<div [pdSash]="width()" [pdSashLimits]="limits" pdSashControls="pane" (pdSashMove)="set($event)" (pdSashCommit)="settle()">`.
 *
 * - `pdSashMove` fires with the wanted width on every step of a drag or key press (already clamped to the limits).
 * - `pdSashStart` fires first when a drag or key press begins, so the consumer can compute the limits for this gesture (the
 *   room the layout plan allows) and hold off re-planning; `pdSashCommit` fires once when it ends (pointer released or
 *   cancelled, key pressed), which is when a layout evaluates, never in the middle of a drag.
 * - `pdSashLimits` is read at every use, so it may follow a signal; `aria-valuemax` follows it.
 */
@Directive({
  selector: '[pdSash]',
  host: {
    role: 'separator',
    'aria-orientation': 'vertical',
    tabindex: '0',
    '[attr.aria-valuenow]': 'pdSash()',
    '[attr.aria-valuemin]': 'pdSashLimits()().min',
    '[attr.aria-valuemax]': 'pdSashLimits()().max',
    '[attr.aria-controls]': 'pdSashControls() || null',
    '[class.pd-sash-dragging]': 'dragging()',
    '(pointerdown)': 'press($event)',
    '(keydown)': 'key($event)',
  },
})
export class PdSash {
  /** The pane's width now, px. */
  readonly pdSash = input.required<number>();
  /** The limits for the gesture in progress (or the idle ones). */
  readonly pdSashLimits = input.required<() => SashLimits>();
  /** The id of the pane this splitter resizes (`aria-controls`). */
  readonly pdSashControls = input<string>('');
  readonly pdSashStart = output<void>();
  readonly pdSashMove = output<number>();
  readonly pdSashCommit = output<void>();

  /** A pointer drag is in progress. */
  readonly dragging = signal(false);
  private readonly el = inject<ElementRef<HTMLElement>>(ElementRef).nativeElement;
  private readonly doc = inject(DOCUMENT);

  private get rtl(): boolean {
    return this.doc.dir === 'rtl';
  }

  protected press(e: PointerEvent): void {
    // The primary button only: a right click opens a menu, a middle click scrolls; neither starts a drag.
    if (e.button !== 0 || this.dragging()) return;
    const startX = e.clientX;
    const startWidth = this.pdSash();
    this.pdSashStart.emit();
    const limits = this.pdSashLimits()();
    const sign = this.rtl ? -1 : 1;
    this.el.setPointerCapture?.(e.pointerId);
    this.dragging.set(true);
    const move = (m: PointerEvent): void => this.pdSashMove.emit(clampSash(startWidth + sign * (m.clientX - startX), limits));
    const up = (): void => {
      this.el.removeEventListener('pointermove', move);
      this.el.removeEventListener('pointerup', up);
      this.el.removeEventListener('pointercancel', up);
      this.dragging.set(false);
      this.pdSashCommit.emit();
    };
    this.el.addEventListener('pointermove', move);
    this.el.addEventListener('pointerup', up);
    this.el.addEventListener('pointercancel', up);
    e.preventDefault();
  }

  protected key(e: KeyboardEvent): void {
    if (e.defaultPrevented || e.altKey || e.ctrlKey || e.metaKey) return;
    const step = e.shiftKey ? SASH_BIG_STEP : SASH_STEP;
    // In right-to-left text the pane is on the right: the arrow away from its edge widens it.
    const grow = this.rtl ? 'ArrowLeft' : 'ArrowRight';
    const shrink = this.rtl ? 'ArrowRight' : 'ArrowLeft';
    if (e.key !== grow && e.key !== shrink && e.key !== 'Home' && e.key !== 'End') return;
    e.preventDefault();
    this.pdSashStart.emit();
    const limits = this.pdSashLimits()();
    const w = this.pdSash();
    const next = e.key === grow ? w + step : e.key === shrink ? w - step : e.key === 'Home' ? limits.min : limits.max;
    this.pdSashMove.emit(clampSash(next, limits));
    this.pdSashCommit.emit();
  }
}
