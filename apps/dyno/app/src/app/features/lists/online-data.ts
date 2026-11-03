import { ChangeDetectionStrategy, Component, inject, output } from '@angular/core';
import { MatCheckbox } from '@angular/material/checkbox';
import { Icon } from '../../shared/icon';
import { IntelStore } from '../../state/intel.store';

/**
 * The switches of the network features (each its own, on by default; there is no Offline mode), the first-run note saying what
 * goes online and to whom, and the DB-IP credit that stays visible. Used on the lists page and in Settings.
 */
@Component({
  selector: 'dy-online-data',
  imports: [MatCheckbox, Icon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @if (store.status(); as s) {
      <section class="card" aria-labelledby="online-h">
        <h2 id="online-h">What goes online</h2>
        <p class="note"><dy-icon name="info" /> <span data-testid="first-run-note">{{ s.firstRunNote }}</span></p>
        <div class="switches" role="group" aria-label="Network features">
          <mat-checkbox data-testid="switch-databases" [checked]="s.geo.switches.locationDatabases" (change)="set('locationDatabases', !s.geo.switches.locationDatabases)">
            Download the location databases (DB-IP, monthly)
          </mat-checkbox>
          <mat-checkbox data-testid="switch-own-location" [checked]="s.geo.switches.ownLocation" (change)="set('ownLocation', !s.geo.switches.ownLocation)">
            Find my location from my public IP (asks Cloudflare, or ipify, for your address)
          </mat-checkbox>
        </div>
        <p class="faint">
          {{ s.geo.databaseOn ? 'Location database of ' + (s.geo.dbDate ?? 'an unknown date') + '.' : 'No location database yet.' }}
          {{ s.geo.homeSource }}.
        </p>
        <p class="faint" data-testid="attribution">{{ s.geo.attribution }}</p>
      </section>
    }
  `,
  styles: `
    :host { display: block; }
    .card { display: grid; gap: var(--sp-2); padding: var(--sp-3); border: 1px solid var(--border); border-radius: var(--r-md); background: var(--surface-2); }
    h2 { margin: 0; font-size: var(--fs-md); }
    .note { margin: 0; display: flex; gap: var(--sp-2); align-items: flex-start; color: var(--text-2); font-size: var(--fs-sm); }
    .switches { display: grid; gap: var(--sp-1); }
    .faint { margin: 0; color: var(--text-3); font-size: var(--fs-xs); }
  `,
})
export class OnlineData {
  protected readonly store = inject(IntelStore);
  /** Raised after a switch changed. */
  readonly changed = output<void>();

  protected set(which: 'locationDatabases' | 'ownLocation', on: boolean): void {
    const s = this.store.status();
    if (!s) return;
    void this.store.setSwitches({ ...s.geo.switches, [which]: on, firstRunNoteSeen: true }).then(() => this.changed.emit());
  }
}
