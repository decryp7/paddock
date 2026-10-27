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
      gap: var(--sp-1);
      padding: var(--sp-6) var(--sp-4);
      text-align: center;
    }
    .icon {
      display: grid;
      place-items: center;
      width: 36px;
      height: 36px;
      margin-bottom: var(--sp-2);
      border-radius: var(--r-lg);
      background: var(--bg-hover);
      color: var(--text-3);
      mat-icon {
        width: 18px;
        height: 18px;
      }
    }
    .title {
      font-weight: var(--fw-medium);
    }
    .text {
      max-width: 360px;
      color: var(--text-2);
      font-size: var(--fs-sm);
    }
    .actions:not(:empty) {
      margin-top: var(--sp-3);
      display: flex;
      gap: var(--sp-2);
    }
  `,
})
export class EmptyState {
  readonly icon = input('info');
  readonly heading = input.required<string>();
  readonly text = input<string>('');
}
