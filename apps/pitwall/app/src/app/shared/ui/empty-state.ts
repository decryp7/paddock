import { ChangeDetectionStrategy, Component, input } from '@angular/core';
import { MatIconModule } from '@angular/material/icon';

/** A friendly "nothing here yet" with what to do next (projected actions). */
@Component({
  selector: 'pw-empty-state',
  imports: [MatIconModule],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="icon"><mat-icon [svgIcon]="'pw:' + icon()" aria-hidden="true" /></div>
    <p class="title">{{ heading() }}</p>
    @if (text()) {
      <p class="text">{{ text() }}</p>
    }
    <div class="actions"><ng-content /></div>
  `,
  styles: `
    :host {
      display: flex;
      flex-direction: column;
      align-items: center;
      justify-content: center;
      gap: var(--pd-space-1);
      padding: var(--pd-space-6) var(--pd-space-4);
      text-align: center;
    }
    .icon {
      display: grid;
      place-items: center;
      width: 36px;
      height: 36px;
      margin-bottom: var(--pd-space-2);
      border-radius: var(--pd-radius-card);
      background: var(--pd-hover);
      color: var(--pd-text-subtle);
      mat-icon {
        width: 18px;
        height: 18px;
      }
    }
    .title {
      font-weight: var(--pd-weight-medium);
    }
    .text {
      max-width: 360px;
      color: var(--pd-text-muted);
      font-size: var(--pd-text-label);
    }
    .actions:not(:empty) {
      margin-top: var(--pd-space-3);
      display: flex;
      gap: var(--pd-space-2);
    }
  `,
})
export class EmptyState {
  readonly icon = input('info');
  readonly heading = input.required<string>();
  readonly text = input<string>('');
}
