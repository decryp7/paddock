import { ChangeDetectionStrategy, Component, inject, signal } from '@angular/core';
import { MatSlideToggle } from '@angular/material/slide-toggle';
import type { NotificationKind } from '../../core/api.types';
import { BACKEND } from '../../core/backend';
import { toCommandError } from '../../core/command-error';
import { PreferencesService } from '../../core/preferences.service';
import { notificationKindLabel } from '../../shared/labels';
import { KIND_GROUPS } from './channels/channels-model';

/** Start at log-in, and which notifications show (the look is the shell's Appearance page). */
@Component({
  selector: 'pc-general-section',
  imports: [MatSlideToggle],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <section class="card">
      <header class="card-header"><h2>Start-up</h2></header>
      <div class="card-body rows">
        <div class="row">
          <div>
            <div class="title">Open Pitcrew at log-in</div>
            <div class="muted small">Shows the tray icon after you log in. The runners do not depend on it: the service keeps them going.</div>
          </div>
          <mat-slide-toggle hideIcon [checked]="autostart() === true" [disabled]="autostart() === null" (change)="setAutostart($event.checked)" aria-label="Open Pitcrew at log-in" />
        </div>
        @if (autostartError(); as e) {
          <p class="tone-danger small" role="alert">{{ e }}</p>
        }
      </div>
    </section>

    <section class="card">
      <header class="card-header">
        <h2>Notifications</h2>
        <span class="meta">shown in this window; stored on this computer</span>
      </header>
      <div class="card-body">
        @for (g of groups; track g.key) {
          <div class="group">
            <div class="group-head">
              <h3>{{ g.title }}</h3>
              <span class="links">
                <button type="button" class="link" (click)="setGroup(g.kinds, true)" [attr.aria-label]="'Turn on all ' + g.title">All</button>
                <button type="button" class="link" (click)="setGroup(g.kinds, false)" [attr.aria-label]="'Turn off all ' + g.title">None</button>
              </span>
            </div>
            <ul class="kinds">
              @for (k of g.kinds; track k) {
                <li>
                  <span>{{ label(k) }}</span>
                  <mat-slide-toggle hideIcon [checked]="prefs.isEnabled(k)" (change)="prefs.setEnabled(k, $event.checked)" [aria-label]="label(k)" />
                </li>
              }
            </ul>
          </div>
        }
      </div>
    </section>
  `,
  styles: `
    :host {
      display: grid;
      gap: var(--space-4);
    }
    h2 {
      font-size: var(--text-md);
    }
    .row {
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: var(--space-6);
    }
    .title {
      font-weight: var(--weight-medium);
    }
    .small {
      font-size: var(--text-sm);
    }
    .card-body {
      max-width: 560px;
    }
    .group + .group {
      margin-top: var(--space-4);
    }
    .group-head {
      display: flex;
      align-items: baseline;
      justify-content: space-between;
      margin-bottom: var(--space-1);
    }
    h3 {
      margin: 0;
      font-size: var(--text-sm);
      font-weight: var(--weight-medium);
      color: var(--text-2);
    }
    .links {
      display: flex;
      gap: var(--space-3);
    }
    .link {
      padding: 0;
      border: 0;
      background: none;
      color: var(--text-2);
      font: inherit;
      font-size: var(--text-sm);
      text-decoration: underline;
      cursor: pointer;
    }
    .link:hover {
      color: var(--text);
    }
    .kinds {
      display: grid;
      margin: 0;
      padding: 0;
      list-style: none;
    }
    .kinds li {
      display: flex;
      align-items: center;
      justify-content: space-between;
      padding: var(--space-2) 0;
      border-bottom: 1px solid var(--border);
    }
  `,
})
export class GeneralSection {
  protected readonly prefs = inject(PreferencesService);
  private readonly backend = inject(BACKEND);

  protected readonly groups = KIND_GROUPS;
  protected readonly autostart = signal<boolean | null>(null);
  protected readonly autostartError = signal<string | null>(null);

  constructor() {
    this.backend.autostart().then(
      (on) => this.autostart.set(on),
      (e) => this.autostartError.set(toCommandError(e).message),
    );
  }

  protected setGroup(kinds: readonly NotificationKind[], on: boolean): void {
    for (const k of kinds) this.prefs.setEnabled(k, on);
  }

  protected label(k: NotificationKind): string {
    return notificationKindLabel(k);
  }

  protected async setAutostart(on: boolean): Promise<void> {
    this.autostartError.set(null);
    try {
      await this.backend.setAutostart(on);
      this.autostart.set(on);
    } catch (e) {
      this.autostartError.set(toCommandError(e).message);
      this.autostart.set(!on);
    }
  }
}
