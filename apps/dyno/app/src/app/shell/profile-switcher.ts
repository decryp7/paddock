import { CdkMenu, CdkMenuItemRadio, CdkMenuTrigger } from '@angular/cdk/menu';
import { ChangeDetectionStrategy, Component, OnInit, inject, input } from '@angular/core';
import { Icon } from '../shared/icon';
import { PROFILE_ICON } from '../shared/labels';
import { RulesStore } from '../state/rules.store';

/** The active profile as a button; its menu switches profile (and offers Pause all rules). */
@Component({
  selector: 'dy-profile-switcher',
  imports: [CdkMenuTrigger, CdkMenu, CdkMenuItemRadio, Icon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <button
      type="button"
      class="btn"
      [class.sm]="compact()"
      [cdkMenuTriggerFor]="menu"
      [disabled]="!rules.loaded()"
      [attr.aria-label]="'Profile: ' + (rules.activeProfile()?.name ?? 'loading') + '. Change profile'"
    >
      <dy-icon [name]="icon(rules.profile())" />
      <span class="name">{{ rules.activeProfile()?.name ?? 'Profile' }}</span>
      @if (rules.paused()) {
        <span class="badge" data-tone="warn">Paused</span>
      }
      <dy-icon name="chevron-down" class="chev" />
    </button>
    <ng-template #menu>
      <div class="menu wide" cdkMenu>
        <div class="menu-label">Profile</div>
        @for (p of rules.profiles(); track p.id) {
          <button type="button" class="menu-item two-line" cdkMenuItemRadio [cdkMenuItemChecked]="p.id === rules.profile()" (cdkMenuItemTriggered)="rules.setProfile(p.id)">
            <dy-icon [name]="icon(p.id)" />
            <span class="text">
              <span>{{ p.name }}</span>
              <small>{{ p.description }}</small>
            </span>
          </button>
        }
        <div class="menu-sep"></div>
        <button type="button" class="menu-item" cdkMenuItemRadio [cdkMenuItemChecked]="rules.paused()" (cdkMenuItemTriggered)="rules.pause(!rules.paused())">
          <dy-icon [name]="rules.paused() ? 'play' : 'pause'" />
          {{ rules.paused() ? 'Resume rules' : 'Pause all rules' }}
        </button>
      </div>
    </ng-template>
  `,
  styles: `
    .name { max-width: 140px; overflow: hidden; text-overflow: ellipsis; }
    .chev { --icon-size: 14px; color: var(--text-3); }
  `,
})
export class ProfileSwitcher implements OnInit {
  protected readonly rules = inject(RulesStore);
  readonly compact = input(false);
  protected readonly icon = (id: keyof typeof PROFILE_ICON) => PROFILE_ICON[id];

  ngOnInit(): void {
    void this.rules.ensure();
  }
}
