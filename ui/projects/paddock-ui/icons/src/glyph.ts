import { ChangeDetectionStrategy, Component, computed, input } from '@angular/core';
import { ICON_GRID, ICON_STROKE } from './outline';
import { PD_ICON_PATHS, type PdIconName } from './paths';

/**
 * Draws one glyph of the generic `pd:` set (or any path data given as `paths`) inline: `<pd-glyph name="refresh" />`. 24 px
 * grid, `currentColor`, round caps. Decorative (`aria-hidden`): the button or label around it carries the name. Apps that
 * use Angular Material draw the same glyphs with `<mat-icon svgIcon="pd:refresh">` after `registerPaddockIcons`.
 */
@Component({
  selector: 'pd-glyph',
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: { 'aria-hidden': 'true' },
  template: `<svg [attr.viewBox]="'0 0 24 24'" [attr.width]="size()" [attr.height]="size()" focusable="false" fill="none" stroke="currentColor" [attr.stroke-width]="strokeWidth()" stroke-linecap="round" stroke-linejoin="round"><path [attr.d]="path()" /></svg>`,
  styles: `
    :host { display: inline-flex; flex: none; align-items: center; justify-content: center; }
    svg { display: block; }
  `,
})
export class PdGlyph {
  /** A glyph of the set ("refresh"; a "pd:" prefix is accepted). */
  readonly name = input<PdIconName | (string & {})>('');
  /** Path data instead of a name: an app's own glyph drawn on the same grid. */
  readonly paths = input<string>('');
  /** Pixels (width and height). */
  readonly size = input(16);
  /**
   * Stroke width on the 24 px grid. Default: the design guide's 1.5 px on the screen at any size, never lighter than the
   * grid's 1.75 (so 16 px glyphs are drawn at 2.25 and 12 px ones at 3).
   */
  readonly stroke = input<number | null>(null);
  protected readonly strokeWidth = computed(() => this.stroke() ?? Math.max(ICON_STROKE, (ICON_GRID * 1.5) / this.size()));
  protected readonly path = computed(() => {
    const p = this.paths();
    if (p) return p;
    const n = this.name().replace(/^pd:/, '');
    return (PD_ICON_PATHS as Readonly<Record<string, string>>)[n] ?? '';
  });
}
