import { ChangeDetectionStrategy, Component, computed, input } from '@angular/core';
import { MatIcon } from '@angular/material/icon';
import { kindIcon } from '../labels';

/** The icon tile of a source kind (decorative: the label is always next to it). */
@Component({
  selector: 'sp-kind-icon',
  imports: [MatIcon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `<span class="tile-icon" aria-hidden="true"><mat-icon [svgIcon]="'sp:' + icon()" /></span>`,
  styles: `
    :host {
      display: inline-flex;
    }
  `,
})
export class KindIcon {
  readonly kind = input.required<string>();
  protected readonly icon = computed(() => kindIcon(this.kind()));
}
