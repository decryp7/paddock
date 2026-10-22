import { ChangeDetectionStrategy, Component, computed, input } from '@angular/core';
import { MatIcon } from '@angular/material/icon';
import type { RunnerStatus } from '../../core/api.types';
import { statusLook } from '../labels';

/** A runner's status in words, with its icon and colour role. */
@Component({
  selector: 'pc-status-badge',
  imports: [MatIcon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <span class="badge" [class]="look().tone" [attr.title]="look().hint">
      <mat-icon [svgIcon]="'pc:' + look().icon" aria-hidden="true" [class.spin]="status() === 'starting'" />
      {{ look().label }}
    </span>
  `,
  styles: `
    :host {
      display: inline-flex;
    }
  `,
})
export class StatusBadge {
  readonly status = input.required<RunnerStatus>();
  protected readonly look = computed(() => statusLook(this.status()));
}
