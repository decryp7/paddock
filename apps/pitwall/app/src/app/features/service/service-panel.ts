import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { Panel } from '../../shared/ui/panel';
import { consentDetail } from './service-consent';
import { ServiceStore } from './service.store';
import { MatButton } from '@angular/material/button';

/**
 * The background service card (Settings). It belongs to the desktop app, not to the server: it renders and works while
 * the server is down, because that is exactly when someone needs it. It says what state the registration is in (running,
 * switched off in Login Items, could not start after an update, not in Login Items, not installed) and offers the matching
 * control (Repair, Install, Remove, Open Login Items). The engine decides the state; this only shows it.
 */
@Component({
  selector: 'pw-service-panel',
  imports: [MatButton, Panel],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <pw-panel id="set-service" heading="Background service" icon="server" sub="So apps can use Pitwall without this window">
      @if (service.service(); as sv) {
        <div class="row">
          <div class="lbl">
            <span class="name">{{ platformLabel(sv.platform) }}</span>
            <p class="hint" data-testid="service-hint">{{ hint(sv) }}</p>
          </div>
          <span class="status" data-testid="service-pill" [attr.data-tone]="tone(sv)">{{ pill(sv) }}</span>
        </div>
        <div class="row">
          <div class="lbl"><span class="name">{{ sv.installed ? 'Remove the service' : 'Run in the background' }}</span><p class="hint" data-testid="service-action-hint">{{ sv.installed ? removeHint(sv.platform) : serviceHint(sv.platform) }}</p></div>
          @if (sv.installed) {
            <button type="button" matButton="outlined" [disabled]="service.isBusy()('uninstall')" (click)="service.uninstall()">{{ service.isBusy()('uninstall') ? 'Removing…' : 'Remove service' }}</button>
          } @else if (!sv.disabledInLoginItems) {
            <button type="button" matButton="filled" [disabled]="!sv.canInstall || service.isBusy()('install')" (click)="service.install()">{{ service.isBusy()('install') ? 'Installing…' : 'Install service' }}</button>
          }
        </div>
        @if (service.canRepair()) {
          <div class="row" data-testid="service-repair-action">
            <div class="lbl"><span class="name">Repair the service</span><p class="hint">Removes the registration, waits until macOS has dropped it and registers the service again. It takes up to a minute.</p></div>
            <button type="button" matButton="filled" data-testid="repair-service" [disabled]="service.isBusy()('repair')" (click)="service.repair()">{{ service.isBusy()('repair') ? 'Repairing…' : 'Repair service' }}</button>
          </div>
        }
        @if (sv.repair; as r) {
          <div class="row" data-testid="service-repair-row">
            <div class="lbl"><span class="name">{{ r.state === 'working' ? 'Repairing the registration' : r.state === 'succeeded' ? 'Registration repaired' : 'Registration could not be repaired' }}</span><p class="hint">{{ r.message }}</p></div>
          </div>
        }
        @if (sv.disabledInLoginItems) {
          <div class="row" data-testid="service-login-items-row">
            <div class="lbl"><span class="name">Switched off in Login Items</span><p class="hint">Switch Pitwall on in System Settings > General > Login Items. Pitwall never does it for you.</p></div>
            <button type="button" matButton="outlined" [disabled]="service.isBusy()('loginItems')" (click)="service.openLoginItems()">Open Login Items</button>
          </div>
        }
      } @else {
        <p class="faint pad">Could not ask the operating system about the service.</p>
      }
    </pw-panel>
  `,
  styles: `
    :host {
      display: block;
      min-width: 0;
    }
    pw-panel {
      scroll-margin-top: var(--pd-space-3);
    }
    .row {
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: var(--pd-space-6);
      min-height: 52px;
      padding: var(--pd-space-3) 0;
      border-top: 1px solid var(--pd-hairline);
      &:first-of-type {
        border-top: none;
        padding-top: 0;
      }
    }
    /* An action keeps its label on one line; the text beside it wraps instead. */
    .row button {
      flex: none;
      min-width: 128px;
      white-space: nowrap;
    }
    .lbl {
      flex: 1 1 auto;
      min-width: 0;
      .name {
        display: block;
        font-weight: var(--pd-weight-medium);
      }
      .hint {
        margin-top: 2px;
        font-size: var(--pd-text-label);
        color: var(--pd-text-muted);
        max-width: 480px;
      }
    }
    .pad {
      padding: var(--pd-space-3) 0;
    }
  `,
})
export class ServicePanel {
  protected readonly service = inject(ServiceStore);

  protected platformLabel(platform: string): string {
    return platform === 'windows' ? 'Windows service' : platform === 'macos' ? 'Login item (launch agent)' : 'systemd user service';
  }

  protected serviceHint(platform: string): string {
    return consentDetail(platform);
  }

  /** What removing does, in the platform's words: the server stops running in the background; models and settings stay. */
  protected removeHint(platform: string): string {
    const what = platform === 'windows' ? 'Windows service' : platform === 'macos' ? 'login item' : 'systemd user service';
    return `Removes the ${what}: Pitwall's server stops running in the background, and apps can reach it only while Pitwall is open. Your models and settings stay.`;
  }

  protected pill(sv: { installed: boolean; running: boolean; disabledInLoginItems: boolean; startFailed: boolean; notInLoginItems: boolean }): string {
    if (sv.installed && sv.running) return 'Running';
    if (sv.disabledInLoginItems) return 'Switched off';
    if (sv.startFailed) return 'Could not start';
    if (sv.notInLoginItems) return 'Not in Login Items';
    return sv.installed ? 'Installed, not running' : 'Not installed';
  }

  protected tone(sv: { installed: boolean; running: boolean; startFailed: boolean; notInLoginItems: boolean }): string {
    if (sv.running && sv.installed) return 'ok';
    if (sv.startFailed || sv.notInLoginItems || sv.installed) return 'warn';
    return 'muted';
  }

  protected hint(sv: { installed: boolean; message: string | null; startFailed: boolean; notInLoginItems: boolean }): string {
    if (sv.startFailed) {
      return `The background service could not start after the update. ${sv.message ?? ''} Press Repair service.`.replace('  ', ' ');
    }
    if (sv.notInLoginItems) {
      return `Pitwall is not listed in System Settings > General > Login Items, so macOS will not start the service. ${sv.message ?? ''} Press Repair service to register it again.`.replace('  ', ' ');
    }
    return sv.message ?? (sv.installed ? "Pitwall's server starts by itself and keeps running when this window is closed." : "Pitwall's server only runs while this app is open.");
  }
}
