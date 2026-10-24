import { ChangeDetectionStrategy, Component, computed, input } from '@angular/core';
import { PdShellIcon } from '@decryp7/paddock-ui/shell';

/** The demo's own icons (`sd:`), 16 px grid, 1.5 px stroke like the shell's. */
const PATHS: Readonly<Record<string, string>> = {
  'sd:lab': 'M6 1.75h4M6.75 1.75v4.5L2.9 12.6a1.1 1.1 0 0 0 .95 1.65h8.3a1.1 1.1 0 0 0 .95-1.65L9.25 6.25v-4.5M4.75 9.5h6.5',
};

/**
 * The demo's icon component (`provideShellPages({ icon })`): its own names, and the shell's for the rest. An app usually
 * wraps Angular Material's registry here (`<mat-icon [svgIcon]="name()" />`).
 */
@Component({
  selector: 'sd-icon',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [PdShellIcon],
  host: { 'aria-hidden': 'true' },
  template: `
    @if (path(); as d) {
      <svg viewBox="0 0 16 16" width="16" height="16" focusable="false">
        <path [attr.d]="d" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" />
      </svg>
    } @else {
      <pd-shell-icon [name]="name()" />
    }
  `,
  styles: `:host { display: inline-flex; width: 16px; height: 16px; } svg { display: block; }`,
})
export class DemoIcon {
  readonly name = input.required<string>();
  protected readonly path = computed(() => PATHS[this.name()] ?? null);
}
