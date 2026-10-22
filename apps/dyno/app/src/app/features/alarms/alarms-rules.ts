import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { AlarmsStore } from '../../state/alarms.store';
import { SettingsStore } from '../../state/settings.store';
import { AlarmSettings } from './alarm-settings';
import { AlarmStats } from './alarm-stats';
import { CustomRules } from './custom-rules';

/** The Rules tab: how the built-in alerts behave, the user's own rules, network alerts and notifications. */
@Component({
  selector: 'dy-alarms-rules',
  imports: [AlarmStats, CustomRules, AlarmSettings],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="stack">
      <dy-alarm-stats />
      <dy-custom-rules />
      <dy-alarm-settings />
    </div>
  `,
  styles: `
    :host { display: block; }
    .stack { display: grid; gap: var(--sp-8); }
  `,
})
export class AlarmsRules {
  constructor() {
    const alarms = inject(AlarmsStore);
    const settings = inject(SettingsStore);
    void alarms.loadStats();
    if (!settings.settings()) void settings.load();
  }
}
