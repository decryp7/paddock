import { ChangeDetectionStrategy, Component, computed, input, output } from '@angular/core';
import type { CommandError } from '../core/api.types';
import { errorHint } from '../core/command-error';
import { Icon } from './icon';
import type { IconName } from './icons';
import { MatButton } from '@angular/material/button';

/**
 * A panel's request states around its content: a skeleton until the first answer, an inline error with Retry when
 * it failed (a banner above stale content when there is some), and an empty state. Each panel owns its own, so one
 * failed request never blanks the page.
 */
@Component({
  selector: 'dy-async',
  imports: [MatButton, Icon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @if (!loaded() && error(); as e) {
      <div class="error" role="alert">
        <dy-icon name="alert" />
        <div class="text">
          <strong>{{ e.message }}</strong>
          @if (hint()) {
            <span>{{ hint() }}</span>
          }
        </div>
        <button type="button" matButton="outlined" (click)="retry.emit()"><dy-icon name="refresh" /> Retry</button>
      </div>
    } @else if (!loaded()) {
      <div class="skeleton-wrap" aria-busy="true" [attr.aria-label]="'Loading ' + what()">
        <ng-content select="[skeleton]" />
        @if (rows() > 0) {
          @for (r of rowList(); track $index) {
            <div class="skeleton" [style.width.%]="r"></div>
          }
        }
      </div>
    } @else {
      @if (error(); as e) {
        <div class="banner" role="alert">
          <dy-icon name="alert" />
          <span class="truncate">{{ e.message }}</span>
          <button type="button" matButton="text" (click)="retry.emit()">Retry</button>
        </div>
      }
      @if (empty()) {
        <div class="empty">
          <dy-icon [name]="emptyIcon()" />
          <strong>{{ emptyTitle() }}</strong>
          @if (emptyText()) {
            <span>{{ emptyText() }}</span>
          }
          <ng-content select="[empty-action]" />
        </div>
      } @else {
        <ng-content />
      }
    }
  `,
  styles: `
    :host { display: block; min-width: 0; }
    .error { display: flex; align-items: flex-start; gap: var(--sp-3); padding: var(--sp-3) var(--sp-4); margin: var(--sp-2) 0; border: 1px solid color-mix(in srgb, var(--bad) 35%, transparent); border-radius: var(--r-md); background: var(--bad-soft); }
    .error > dy-icon { color: var(--bad); margin-top: 1px; }
    .error .text { flex: 1; display: grid; gap: 2px; min-width: 0; }
    .error .text span { color: var(--text-2); font-size: var(--fs-sm); }
    .banner { display: flex; align-items: center; gap: var(--sp-2); padding: var(--sp-1) var(--sp-1) var(--sp-1) var(--sp-3); margin-bottom: var(--sp-2); border-radius: var(--r-md); background: var(--bad-soft); color: var(--text-1); font-size: var(--fs-sm); }
    .banner dy-icon { --icon-size: 15px; color: var(--bad); }
    .banner span { flex: 1; }
    .skeleton-wrap { display: grid; gap: var(--sp-3); padding: var(--sp-1) 0; }
    .skeleton { height: 12px; }
    .empty { display: grid; justify-items: center; gap: var(--sp-1); padding: var(--sp-8) var(--sp-4); text-align: center; color: var(--text-2); }
    .empty dy-icon { --icon-size: 28px; color: var(--text-3); margin-bottom: var(--sp-1); }
    .empty strong { color: var(--text-1); font-weight: var(--fw-medium); }
    .empty span { font-size: var(--fs-sm); max-width: 44ch; }
  `,
})
export class AsyncState {
  readonly loaded = input.required<boolean>();
  readonly error = input<CommandError | null>(null);
  readonly empty = input(false);
  readonly emptyTitle = input('Nothing here');
  readonly emptyText = input('');
  readonly emptyIcon = input<IconName>('check');
  /** Generic skeleton bars (0 when a custom `[skeleton]` is projected). */
  readonly rows = input(3);
  readonly what = input('content');
  readonly retry = output<void>();

  protected readonly hint = computed(() => {
    const e = this.error();
    return e ? errorHint(e.code) : null;
  });
  protected readonly rowList = computed(() => Array.from({ length: this.rows() }, (_, i) => [92, 76, 84, 58, 70][i % 5]!));
}
