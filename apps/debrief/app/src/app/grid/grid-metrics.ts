import { Injectable, signal } from '@angular/core';
import { DEFAULT_CHAR_PX } from './message-column';

/**
 * The width of one character of the grid's time cell in the font the theme gives it, px. The grid measures it (canvas
 * `measureText` on the cell's computed font) when it shows and whenever the theme or the fonts change; the layout plan reads it
 * to size the Events view's minimum. Until measured it is `DEFAULT_CHAR_PX`.
 */
@Injectable({ providedIn: 'root' })
export class GridMetrics {
  private readonly width = signal(DEFAULT_CHAR_PX);
  /** One character of the time cell, px. */
  readonly charPx = this.width.asReadonly();

  /** Measures a monospace sample in the font of `cell` and keeps the width of one character. */
  measure(cell: HTMLElement, canvas: HTMLCanvasElement = cell.ownerDocument.createElement('canvas')): void {
    const style = cell.ownerDocument.defaultView?.getComputedStyle(cell);
    const ctx = canvas.getContext('2d');
    if (!style || !ctx) return;
    ctx.font = `${style.fontStyle} ${style.fontWeight} ${style.fontSize} ${style.fontFamily}`;
    const sample = '0123456789';
    const px = ctx.measureText(sample).width / sample.length;
    // Rounded up to a hundredth: a re-measure that finds the same font must not look like a change.
    if (Number.isFinite(px) && px > 0) this.width.set(Math.ceil(px * 100) / 100);
  }
}
