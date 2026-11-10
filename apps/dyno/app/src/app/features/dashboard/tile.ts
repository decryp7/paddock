import { ChangeDetectionStrategy, Component, input } from '@angular/core';
import { Icon } from '../../shared/icon';
import type { IconName } from '../../shared/icons';

/**
 * A dashboard tile: icon and title, a slot for a badge, and the body. When the platform cannot provide the reading
 * the body is replaced by "Not available on this computer" with the engine's reason; until the first update a skeleton
 * shows. Nothing in a tile ever shows an invented zero.
 */
@Component({
  selector: 'dy-tile',
  imports: [Icon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: { role: 'group', '[attr.aria-label]': 'title()' },
  template: `
    <header>
      <dy-icon [name]="icon()" />
      <h2>{{ title() }}</h2>
      <span class="spacer"></span>
      <ng-content select="[badge]" />
    </header>
    @if (reason(); as why) {
      <div class="na">
        <strong>Not available on this computer</strong>
        <span>{{ why }}</span>
      </div>
    } @else if (!ready()) {
      <div class="loading" role="status" aria-busy="true" aria-label="Loading">
        <div class="skeleton big"></div>
        <div class="skeleton"></div>
        <div class="skeleton short"></div>
      </div>
    } @else {
      <div class="body"><ng-content /></div>
    }
  `,
  styles: `
    :host { display: flex; flex-direction: column; min-width: 0; min-height: 168px; padding: var(--sp-4); gap: var(--sp-3); background: var(--surface-2); border: 1px solid var(--border); border-radius: var(--r-lg); box-shadow: var(--shadow-1); }
    header { display: flex; align-items: center; gap: var(--sp-2); min-height: 22px; color: var(--text-2); }
    header dy-icon { --icon-size: 16px; color: var(--text-3); }
    h2 { font-size: var(--fs-sm); font-weight: var(--fw-semibold); letter-spacing: 0.02em; color: var(--text-2); }
    .spacer { flex: 1; }
    .body { display: grid; gap: var(--sp-3); min-width: 0; flex: 1; align-content: start; }
    .na { display: grid; gap: var(--sp-1); align-content: center; flex: 1; padding: var(--sp-2) 0; color: var(--text-2); font-size: var(--fs-sm); }
    .na strong { color: var(--text-1); font-weight: var(--fw-medium); font-size: var(--fs-base); }
    .loading { display: grid; gap: var(--sp-2); }
    .big { height: 28px; width: 50%; }
    .short { width: 60%; }
  `,
})
export class Tile {
  readonly title = input.required<string>();
  readonly icon = input.required<IconName>();
  /** Why the platform cannot provide this reading (shown instead of the body), or `null`. */
  readonly reason = input<string | null>(null);
  /** False until the first live update arrived. */
  readonly ready = input(true);
}
