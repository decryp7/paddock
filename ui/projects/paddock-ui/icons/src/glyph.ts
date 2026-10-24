import { ChangeDetectionStrategy, Component, computed, input } from '@angular/core';
import { ICON_STROKE, type OutlineDot } from './outline';
import { PD_ICON_DOTS, PD_ICON_PATHS, type PdIconName } from './paths';

/**
 * Draws one glyph of the generic `pd:` set (or any path data given as `paths`) inline: `<pd-glyph name="refresh" />`. 24 px
 * grid, `currentColor`, round caps. Decorative (`aria-hidden`): the button or label around it carries the name. Apps that
 * use Angular Material draw the same glyphs with `<mat-icon svgIcon="pd:refresh">` after `registerPaddockIcons`.
 */
@Component({
  selector: 'pd-glyph',
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: { 'aria-hidden': 'true' },
  template: `<svg [attr.viewBox]="'0 0 24 24'" [attr.width]="size()" [attr.height]="size()" focusable="false" fill="none" stroke="currentColor" [attr.stroke-width]="strokeWidth()" stroke-linecap="round" stroke-linejoin="round">@if (path()) {<path [attr.d]="path()" vector-effect="non-scaling-stroke" />}@for (d of dots(); track $index) {<circle [attr.cx]="d[0]" [attr.cy]="d[1]" [attr.r]="d[2]" fill="currentColor" stroke="none" />}</svg>`,
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
  /** Filled dots (`[cx, cy, r]` on the grid) of an app's own `paths` glyph; a named glyph brings its own. */
  readonly dotList = input<readonly OutlineDot[]>([], { alias: 'dots' });
  /** Stroke width in screen pixels at any size (the glyph's strokes do not scale with `size`). Default: the guide's 1.5 px. */
  readonly stroke = input<number | null>(null);
  protected readonly strokeWidth = computed(() => this.stroke() ?? ICON_STROKE);
  protected readonly dots = computed(() => {
    if (this.paths()) return this.dotList();
    const n = this.name().replace(/^pd:/, '') as PdIconName;
    return PD_ICON_DOTS[n] ?? [];
  });
  protected readonly path = computed(() => {
    const p = this.paths();
    if (p) return p;
    const n = this.name().replace(/^pd:/, '');
    return (PD_ICON_PATHS as Readonly<Record<string, string>>)[n] ?? '';
  });
}
