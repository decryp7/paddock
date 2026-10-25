import { ChangeDetectionStrategy, Component, inject, input } from '@angular/core';
import { BACKEND } from '../../core/backend';

/** Pitcrew's app icon (same-origin SVG): the macOS tile on macOS, the Windows tile elsewhere. Decorative. */
@Component({
  selector: 'pc-brand-logo',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `<img [src]="src" alt="" [width]="size()" [height]="size()" draggable="false" />`,
  styles: `
    :host {
      display: inline-flex;
      flex: none;
    }
    img {
      display: block;
      user-select: none;
    }
  `,
})
export class BrandLogo {
  readonly size = input(24);
  protected readonly src = `brand/pitcrew-${inject(BACKEND).platform() === 'macos' ? 'macos' : 'windows'}.svg`;
}
