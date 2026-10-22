import { ChangeDetectionStrategy, Component, inject, signal } from '@angular/core';
import { MatIcon } from '@angular/material/icon';
import { MatSlideToggle } from '@angular/material/slide-toggle';
import type { NotificationKind } from '../../core/api.types';
import { BACKEND } from '../../core/backend';
import { toCommandError } from '../../core/command-error';
import { PreferencesService } from '../../core/preferences.service';
import { ThemeService, type ThemePreference } from '../../core/theme.service';
import { NOTIFICATION_KINDS, notificationKindLabel } from '../../shared/labels';

/** Theme, start at log-in, and which notifications show in this window. */
@Component({
  selector: 'pc-general-section',
  imports: [MatIcon, MatSlideToggle],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <section class="card">
      <header class="card-header"><h2>Appearance</h2></header>
      <div class="card-body">
        <div class="themes" role="radiogroup" aria-label="Theme">
          @for (t of themes; track t.key) {
            <button type="button" role="radio" class="theme" [attr.aria-checked]="theme.preference() === t.key" (click)="theme.setPreference(t.key)">
              <span class="preview" [class]="t.key"><span></span><span></span></span>
              <span class="label"><mat-icon [svgIcon]="'pc:' + t.icon" aria-hidden="true" />{{ t.label }}</span>
            </button>
          }
        </div>
      </div>
    </section>

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
        <ul class="kinds">
          @for (k of kinds; track k) {
            <li>
              <span>{{ label(k) }}</span>
              <mat-slide-toggle hideIcon [checked]="prefs.isEnabled(k)" (change)="prefs.setEnabled(k, $event.checked)" [aria-label]="label(k)" />
            </li>
          }
        </ul>
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
    .themes {
      display: flex;
      gap: var(--space-3);
    }
    .theme {
      display: grid;
      gap: var(--space-2);
      padding: var(--space-2);
      border: 1px solid var(--border-strong);
      border-radius: var(--radius-md);
      background: none;
      color: var(--text);
      font: inherit;
      cursor: pointer;
    }
    .theme[aria-checked='true'] {
      border-color: var(--accent);
      box-shadow: 0 0 0 1px var(--accent);
    }
    .preview {
      display: grid;
      grid-template-columns: 30% 1fr;
      width: 132px;
      height: 76px;
      border-radius: var(--radius-sm);
      overflow: hidden;
      border: 1px solid var(--border);
    }
    .preview.light span:first-child {
      background: #ececf0;
    }
    .preview.light span:last-child {
      background: #fbfbfc;
    }
    .preview.dark span:first-child {
      background: #111216;
    }
    .preview.dark span:last-child {
      background: #1b1c21;
    }
    .preview.system span:first-child {
      background: linear-gradient(135deg, #ececf0 50%, #111216 50%);
    }
    .preview.system span:last-child {
      background: linear-gradient(135deg, #fbfbfc 50%, #1b1c21 50%);
    }
    .label {
      display: inline-flex;
      align-items: center;
      gap: 6px;
      font-weight: var(--weight-medium);
    }
    .label mat-icon {
      width: 15px;
      height: 15px;
      color: var(--text-2);
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
    .kinds {
      display: grid;
      grid-template-columns: 1fr 1fr;
      gap: 0 var(--space-8);
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
  protected readonly theme = inject(ThemeService);
  protected readonly prefs = inject(PreferencesService);
  private readonly backend = inject(BACKEND);

  protected readonly themes: { key: ThemePreference; label: string; icon: string }[] = [
    { key: 'system', label: 'Follow system', icon: 'monitor' },
    { key: 'light', label: 'Light', icon: 'sun' },
    { key: 'dark', label: 'Dark', icon: 'moon' },
  ];
  protected readonly kinds = NOTIFICATION_KINDS;
  protected readonly autostart = signal<boolean | null>(null);
  protected readonly autostartError = signal<string | null>(null);

  constructor() {
    this.backend.autostart().then(
      (on) => this.autostart.set(on),
      (e) => this.autostartError.set(toCommandError(e).message),
    );
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
