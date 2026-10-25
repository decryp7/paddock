import { ChangeDetectionStrategy, Component, input } from '@angular/core';
import { MatIcon } from '@angular/material/icon';
import { BrandLogo } from './brand-logo';

/** An empty state: what is missing and (projected) what to do about it. */
@Component({
  selector: 'pc-empty-state',
  imports: [MatIcon, BrandLogo],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="empty">
      @if (brand()) {
        <pc-brand-logo [size]="56" class="brand" />
      } @else {
        <mat-icon [svgIcon]="'pc:' + icon()" aria-hidden="true" />
      }
      <h3>{{ title() }}</h3>
      @if (text()) {
        <p>{{ text() }}</p>
      }
      <div class="btn-row actions"><ng-content /></div>
    </div>
  `,
  styles: `
    .brand {
      margin-bottom: var(--space-2);
      filter: drop-shadow(0 6px 16px rgb(212 68 13 / 0.25));
    }
    .actions {
      margin-top: var(--space-2);
      justify-content: center;
    }
    .actions:empty {
      display: none;
    }
  `,
})
export class EmptyState {
  readonly icon = input('runner');
  /** Shows Pitcrew's app icon instead (first-run states). */
  readonly brand = input(false);
  readonly title = input.required<string>();
  readonly text = input<string | null>(null);
}
