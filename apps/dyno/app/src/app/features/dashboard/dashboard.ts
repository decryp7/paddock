import { ChangeDetectionStrategy, Component, computed, inject } from '@angular/core';
import { RouterLink } from '@angular/router';
import { NormalCard } from './normal-card';
import { WidgetBoard } from '../widgets/widget-board';
import { formatDuration } from '../../shared/format';
import { Icon } from '../../shared/icon';
import { LiveStore } from '../../state/live.store';
import { MatButton } from '@angular/material/button';

/**
 * The home screen, made of widgets on the shared grid: the default layout shows how the computer is doing right now, what is
 * using it, how it got here and what Dyno noticed; the user arranges it in edit mode. Each widget's frame draws its loading,
 * empty and error state, so one failed request never blanks the page.
 */
@Component({
  selector: 'dy-dashboard',
  imports: [MatButton, RouterLink, Icon, WidgetBoard, NormalCard],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="page">
      <header class="page-head">
        <div class="titles">
          <h1>Dashboard</h1>
          <p class="num">{{ subtitle() }}</p>
        </div>
        <a matButton="outlined" routerLink="/processes"><dy-icon name="processes" /> Open Processes</a>
      </header>

      <dy-normal-card />

      <dy-widget-board />
    </div>
  `,
  styles: `
    :host { display: block; }
    .page-head { align-items: center; margin-bottom: var(--sp-4); }
    .page-head p { color: var(--text-3); }
  `,
})
export class Dashboard {
  protected readonly live = inject(LiveStore);
  protected readonly subtitle = computed(() => {
    const s = this.live.system();
    if (!s) return 'Connecting to the Dyno agent…';
    return `up ${formatDuration(s.uptimeSecs)}`;
  });
}
