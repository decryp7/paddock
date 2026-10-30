import { ChangeDetectionStrategy, Component, input } from '@angular/core';
import { MatIcon } from '@angular/material/icon';
import { PdShellIcon } from '@decryp7/paddock-ui/shell';

/**
 * The icon component of the pages frame (`provideShellPages({ icon })`): Pitcrew's own icons (`pc:` names, registered with
 * Angular Material's registry as literals) and the shell's (`pd:`) for the rest.
 */
@Component({
  selector: 'pc-icon',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [MatIcon, PdShellIcon],
  host: { 'aria-hidden': 'true' },
  template: `
    @if (name().startsWith('pc:')) {
      <mat-icon [svgIcon]="name()" />
    } @else {
      <pd-shell-icon [name]="name()" />
    }
  `,
  styles: `
    :host {
      display: inline-flex;
      width: 16px;
      height: 16px;
    }
    mat-icon {
      width: 16px;
      height: 16px;
    }
  `,
})
export class PitcrewIcon {
  readonly name = input.required<string>();
}
