import { ChangeDetectionStrategy, Component, OnInit, computed, inject } from '@angular/core';
import { RouterLink } from '@angular/router';
import { WidgetBoard } from '../widgets/widget-board';
import { formatDuration } from '../../shared/format';
import { Icon } from '../../shared/icon';
import { LiveStore } from '../../state/live.store';
import { RulesStore } from '../../state/rules.store';

/**
 * The home screen, made of widgets: the default layout shows how the PC is doing right now, what is using it, how it
 * got here and what Dyno did about it; the user edits, saves and resets it. Every widget owns its loading, empty and error
 * state, so one failed request never blanks the page.
 */
@Component({
  selector: 'dy-dashboard',
  imports: [RouterLink, Icon, WidgetBoard],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="page">
      <header class="page-head">
        <div class="titles">
          <h1>Dashboard</h1>
          <p class="num">{{ subtitle() }}</p>
        </div>
        <a class="btn" routerLink="/processes"><dy-icon name="processes" /> Open task manager</a>
        <button type="button" class="btn" [disabled]="!rules.loaded() || rules.saving()" (click)="rules.pause(!rules.paused())">
          <dy-icon [name]="rules.paused() ? 'play' : 'pause'" /> {{ rules.paused() ? 'Resume rules' : 'Pause rules' }}
        </button>
      </header>

      <dy-widget-board />
    </div>
  `,
  styles: `
    :host { display: block; container-type: inline-size; container-name: dash; }
    .page-head { align-items: center; margin-bottom: var(--sp-4); }
    .page-head p { color: var(--text-3); }
  `,
})
export class Dashboard implements OnInit {
  protected readonly live = inject(LiveStore);
  protected readonly rules = inject(RulesStore);
  protected readonly subtitle = computed(() => {
    const s = this.live.system();
    const profile = this.rules.activeProfile()?.name;
    if (!s) return 'Connecting to the Dyno agent…';
    return `${profile ? `${profile} profile · ` : ''}up ${formatDuration(s.uptimeSecs)}${this.rules.paused() ? ' · rules paused' : ''}`;
  });

  ngOnInit(): void {
    void this.rules.ensure();
  }
}
