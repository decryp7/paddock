import { ChangeDetectionStrategy, Component, input } from '@angular/core';

/**
 * Debrief's app icon: the family's macOS master (`public/brand/debrief-macos.svg`, installed from Paddock's `icons/debrief` by
 * `icons/tools/install.mjs`; the CSP allows local images only). Decorative (hidden from assistive tech) unless `label` is set.
 */
@Component({
  selector: 'debrief-app-logo',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `<img
    src="brand/debrief-macos.svg"
    [attr.width]="size()"
    [attr.height]="size()"
    [attr.alt]="label()"
    [attr.aria-label]="label() || null"
    [attr.aria-hidden]="label() ? null : 'true'"
    [attr.role]="label() ? 'img' : null"
    draggable="false"
  />`,
  styles: `
    :host {
      display: inline-flex;
      flex: none;
    }
    img {
      display: block;
    }
  `,
})
export class AppLogo {
  readonly size = input(20);
  /** Accessible name; when empty the logo is decorative. */
  readonly label = input('');
}
