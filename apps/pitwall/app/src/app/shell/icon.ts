import { ChangeDetectionStrategy, Component, computed, input } from '@angular/core';
import { MatIcon } from '@angular/material/icon';
import { PdShellIcon } from '@decryp7/paddock-ui/shell';

/** The icon component of the pages frame: Pitwall's own icons (`pw:`, Material's registry) and the shell's (`pd:`). */
@Component({
  selector: 'pw-shell-icon',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [MatIcon, PdShellIcon],
  host: { 'aria-hidden': 'true' },
  template: `
    @if (own()) {
      <mat-icon [svgIcon]="name()" aria-hidden="true" />
    } @else {
      <pd-shell-icon [name]="name()" />
    }
  `,
  styles: `
    :host { display: inline-flex; width: 16px; height: 16px; }
    mat-icon { width: 16px; height: 16px; }
  `,
})
export class PitwallIcon {
  readonly name = input.required<string>();
  protected readonly own = computed(() => this.name().startsWith('pw:'));
}
