import { afterRenderEffect, ChangeDetectionStrategy, Component, computed, ElementRef, inject, input } from '@angular/core';
import { MatIconModule, MatIconRegistry } from '@angular/material/icon';
import { DomSanitizer } from '@angular/platform-browser';
import { ICON_GRID, ICON_STROKE } from './outline';
import { PD_ICON_DOTS, type PdIconName } from './paths';
import { ensurePaddockIcons, isPdIcon, pdIconId } from './register';

/**
 * Draws one glyph of the generic `pd:` set: `<pd-glyph name="refresh" />`, which is `<mat-icon svgIcon="pd:refresh">` with the
 * family's size and nothing else (rule 47). The set is registered as literals with Material's icon registry
 * (`providePaddockIcons`, never fetched); 24 px grid, `currentColor`, 1.5 px strokes, solid dots. Decorative (`aria-hidden`):
 * the button or label around it carries the name. A name that is not in the set draws an empty box of the same size.
 */
@Component({
  selector: 'pd-glyph',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [MatIconModule],
  host: { 'aria-hidden': 'true', '[style.width.px]': 'size()', '[style.height.px]': 'size()' },
  template: `@if (id(); as svg) {<mat-icon [svgIcon]="svg" [style.width.px]="size()" [style.height.px]="size()" />}`,
  styles: `
    :host { display: inline-flex; flex: none; align-items: center; justify-content: center; }
    mat-icon { display: block; flex: none; }
  `,
})
export class PdGlyph {
  /** A glyph of the set ("refresh"; a "pd:" prefix is accepted). */
  readonly name = input<PdIconName | (string & {})>('');
  /** Pixels (width and height). */
  readonly size = input(16);
  protected readonly id = computed(() => (isPdIcon(this.name()) ? pdIconId(this.name()) : ''));

  private readonly host = inject<ElementRef<HTMLElement>>(ElementRef);

  constructor() {
    ensurePaddockIcons(inject(MatIconRegistry), inject(DomSanitizer), true);
    // Dots are as heavy as the strokes: never thinner than half the stroke's width on the screen, whatever the size (a 1.5 px
    // line at 14 px would otherwise outweigh a 0.9 unit dot). The registered literal carries the authored radius.
    afterRenderEffect(() => {
      const id = this.id();
      const min = (ICON_STROKE / 2) * (ICON_GRID / this.size());
      const dots = id ? (PD_ICON_DOTS[id.slice(3) as PdIconName] ?? []) : [];
      this.host.nativeElement.querySelectorAll('circle').forEach((c, i) => {
        const r = dots[i]?.[2];
        if (r !== undefined) c.setAttribute('r', String(Math.max(r, min)));
      });
    });
  }
}
