import { ChangeDetectionStrategy, Component, computed, input } from '@angular/core';
import { MatIconModule } from '@angular/material/icon';
import type { IconName } from './icons';

/**
 * One of Dyno's SVG icons (`<dy-icon name="cpu" />`). Decorative by default (hidden from screen readers); pass
 * `label` when the icon carries meaning on its own. Size with the `--icon-size` custom property (default 18 px).
 */
@Component({
  selector: 'dy-icon',
  imports: [MatIconModule],
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: {
    '[attr.aria-hidden]': 'label() ? null : "true"',
    '[attr.role]': 'label() ? "img" : null',
    '[attr.aria-label]': 'label()',
  },
  template: `<mat-icon [svgIcon]="svg()" />`,
  styles: `
    :host { display: inline-flex; flex: none; width: var(--icon-size, 18px); height: var(--icon-size, 18px); line-height: 0; }
    /* Outranks a host's own icon rules (a menu item gives .mat-icon 24 px and a margin): the icon is always the size of its box. */
    :host mat-icon { width: 100%; height: 100%; margin: 0; font-size: 0; }
  `,
})
export class Icon {
  readonly name = input.required<IconName>();
  readonly label = input<string | null>(null);
  protected readonly svg = computed(() => `dy:${this.name()}`);
}
