import { ChangeDetectionStrategy, Component, computed, input } from '@angular/core';
import { PdGlyph } from './glyph';
import type { PdIconName } from './paths';

/** A status a glyph can stand for. */
export type PdStatus = 'error' | 'warning' | 'info' | 'success';

/**
 * The one status → glyph map of the suite (review 3 P2-4, rule 40): error = circle-×, warning = triangle, info = circle-i,
 * success = check. The shape carries the status for readers who cannot tell the colours apart, so a surface never picks
 * its own: it draws `<pd-status-glyph status="…">` or asks `statusGlyph`. The lint in status.spec.ts refuses a status
 * glyph written by name in any template.
 */
export const PD_STATUS_GLYPHS: Readonly<Record<PdStatus, PdIconName>> = Object.freeze({ error: 'error', warning: 'alert', info: 'info', success: 'check' });

/** The glyph of a status (`statusGlyph('error')` is `'error'`, the circle-×). */
export function statusGlyph(status: PdStatus): PdIconName {
  return PD_STATUS_GLYPHS[status];
}

/**
 * The glyph of a status: `<pd-status-glyph status="error" [size]="14" />`. Decorative like `pd-glyph` (the text beside it
 * says what happened); the colour comes from the surrounding text (`currentColor`).
 */
@Component({
  selector: 'pd-status-glyph',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [PdGlyph],
  host: { 'aria-hidden': 'true', '[attr.data-status]': 'status()' },
  template: `<pd-glyph [name]="name()" [size]="size()" />`,
  styles: `:host { display: inline-flex; flex: none; }`,
})
export class PdStatusGlyph {
  readonly status = input.required<PdStatus>();
  /** Pixels (width and height); 14 at least (the icon floor). */
  readonly size = input(16);
  protected readonly name = computed(() => statusGlyph(this.status()));
}
