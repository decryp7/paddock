import { ChangeDetectionStrategy, Component, computed, inject } from '@angular/core';
import type { AlarmConfig, CommandError, NetworkAlerts, Settings } from '../../core/api.types';
import { AsyncState } from '../../shared/async-state';
import { Notifier } from '../../shared/notifier';
import { MatSlideToggle } from '@angular/material/slide-toggle';
import { AlarmsStore } from '../../state/alarms.store';
import { SettingsStore } from '../../state/settings.store';
import { clockToMinutes, defaultAlarmConfig, minutesToClock, withQuiet } from './alarms-model';
import { TimeField } from '../../shared/time-field';

/** Network alarms and how alarms reach you: desktop notifications and quiet hours. Every change saves at once. */
@Component({
  selector: 'dy-alarm-settings',
  imports: [TimeField, AsyncState, MatSlideToggle],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <dy-async [loaded]="!!settings.settings()" [error]="settings.error()" what="alarm settings" [rows]="3" (retry)="settings.load()">
      <section aria-labelledby="net-h" class="block">
        <div class="sec-head"><h2 class="h2" id="net-h">Network alerts</h2><p class="lead">Both are off until you turn them on.</p></div>
        <div class="rows">
          <div class="srow">
            <div class="txt"><span class="name">New exposed listener</span><span class="help">A program starts listening on every network interface, so other devices can reach it.</span></div>
            <mat-slide-toggle [checked]="net().newExposedListener" [disabled]="alarms.saving()" aria-label="New exposed listener" (change)="setNet({ newExposedListener: $event.checked })" />
          </div>
          <div class="srow">
            <div class="txt"><span class="name">New remote host</span><span class="help">A program connects to a host it has not talked to before.</span></div>
            <mat-slide-toggle [checked]="net().newRemoteHost" [disabled]="alarms.saving()" aria-label="New remote host" (change)="setNet({ newRemoteHost: $event.checked })" />
          </div>
        </div>
      </section>

      <section aria-labelledby="notif-h" class="block">
        <div class="sec-head"><h2 class="h2" id="notif-h">Notifications</h2><p class="lead">Muted rules and quiet hours are always respected.</p></div>
        <div class="rows">
          <div class="srow">
            <div class="txt"><span class="name">Desktop notifications</span><span class="help">Show a notification when a new alarm is raised.</span></div>
            <mat-slide-toggle [checked]="settings.settings()?.notifications ?? false" [disabled]="alarms.saving()" aria-label="Desktop notifications" (change)="setNotifications($event.checked)" />
          </div>
          <div class="srow">
            <div class="txt"><span class="name">Quiet hours</span><span class="help">No notifications between these times. Alarms are still recorded.</span></div>
            <mat-slide-toggle [checked]="quiet().enabled" [disabled]="alarms.saving()" aria-label="Quiet hours" (change)="setQuiet({ enabled: $event.checked })" />
          </div>
          <div class="srow" [class.off]="!quiet().enabled">
            <div class="txt"><span class="name">From and until</span><span class="help">An end before the start runs past midnight.</span></div>
            <div class="times">
              <dy-time-field label="Quiet hours start" [disabled]="!quiet().enabled" [value]="clock(quiet().startMinute)" (timeChange)="setTime('startMinute', $event)" />
              <span class="faint" aria-hidden="true">to</span>
              <dy-time-field label="Quiet hours end" [disabled]="!quiet().enabled" [value]="clock(quiet().endMinute)" (timeChange)="setTime('endMinute', $event)" />
            </div>
          </div>
          <div class="srow" [class.off]="!quiet().enabled">
            <div class="txt"><span class="name">Critical alarms still notify</span><span class="help">Critical alarms break through quiet hours.</span></div>
            <mat-slide-toggle [checked]="quiet().criticalBypass" [disabled]="alarms.saving() || !quiet().enabled" aria-label="Critical alarms still notify" (change)="setQuiet({ criticalBypass: $event.checked })" />
          </div>
        </div>
      </section>
    </dy-async>
  `,
  styles: `
    :host { display: block; }
    .block { margin-bottom: var(--sp-6); max-width: 880px; }
    .sec-head { display: grid; gap: 2px; margin-bottom: var(--sp-2); }
    .h2 { margin: 0; font: var(--fw-semibold) var(--fs-md) / 1.3 var(--font-sans); }
    .lead { margin: 0; color: var(--text-3); font-size: var(--fs-sm); }
    .srow { display: flex; align-items: center; justify-content: space-between; gap: var(--sp-6); padding: var(--sp-3) 0; border-top: 1px solid var(--border-subtle); }
    .srow.off { opacity: 0.55; }
    .txt { display: grid; gap: 2px; min-width: 0; }
    .name { font-size: var(--fs-base); }
    .help { color: var(--text-3); font-size: var(--fs-sm); }
    .times { display: flex; align-items: center; gap: var(--sp-2); flex: none; }
    .times input { width: 132px; font-variant-numeric: tabular-nums; }
    .faint { color: var(--text-3); }
  `,
})
export class AlarmSettings {
  protected readonly settings = inject(SettingsStore);
  protected readonly alarms = inject(AlarmsStore);
  private readonly notifier = inject(Notifier);
  protected readonly net = computed<NetworkAlerts>(() => this.settings.settings()?.networkAlerts ?? { newExposedListener: false, newRemoteHost: false });
  protected readonly config = computed<AlarmConfig>(() => this.settings.settings()?.alarms ?? defaultAlarmConfig(-new Date().getTimezoneOffset()));
  protected readonly quiet = computed(() => this.config().quiet);
  protected readonly clock = minutesToClock;

  private async save(change: (s: Settings) => Settings): Promise<void> {
    const error: CommandError | null = await this.alarms.updateSettings(change);
    if (error) this.notifier.error(error);
  }

  protected setNet(patch: Partial<NetworkAlerts>): Promise<void> {
    return this.save((s) => ({ ...s, networkAlerts: { ...this.net(), ...patch } }));
  }

  protected setNotifications(on: boolean): Promise<void> {
    return this.save((s) => ({ ...s, notifications: on }));
  }

  protected setQuiet(patch: Partial<AlarmConfig['quiet']>): Promise<void> {
    return this.save((s) => ({ ...s, alarms: withQuiet(this.config(), patch) }));
  }

  protected async setTime(field: 'startMinute' | 'endMinute', text: string): Promise<void> {
    const minute = clockToMinutes(text);
    if (minute === null) return;
    await this.setQuiet({ [field]: minute });
  }
}
