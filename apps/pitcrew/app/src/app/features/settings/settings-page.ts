import { ChangeDetectionStrategy, Component, computed, inject, input, signal } from '@angular/core';
import { MatIcon } from '@angular/material/icon';
import { Router } from '@angular/router';
import type { AppUpdateStatus } from '../../core/api.types';
import { BACKEND } from '../../core/backend';
import { toCommandError } from '../../core/command-error';
import { BrandLogo } from '../../shared/ui/brand-logo';
import { StatusStore } from '../../state/status.store';
import { ChannelsSection } from './channels/channels-section';
import { DefaultsSection } from './defaults-section';
import { GeneralSection } from './general-section';
import { ServiceSection } from './service-section';
import { TokensSection } from './tokens-section';

export type SettingsSection = 'general' | 'service' | 'defaults' | 'channels' | 'tokens' | 'diagnostics' | 'about';

const SECTIONS: { key: SettingsSection; label: string; icon: string }[] = [
  { key: 'general', label: 'General', icon: 'settings' },
  { key: 'service', label: 'Service', icon: 'server' },
  { key: 'defaults', label: 'Defaults', icon: 'gauge' },
  { key: 'channels', label: 'Notification channels', icon: 'bell' },
  { key: 'tokens', label: 'Gitea tokens', icon: 'key' },
  { key: 'diagnostics', label: 'Diagnostics', icon: 'package' },
  { key: 'about', label: 'About', icon: 'info' },
];

/** Settings, one section at a time (`?section=`). */
@Component({
  selector: 'pc-settings-page',
  imports: [MatIcon, BrandLogo, GeneralSection, ServiceSection, DefaultsSection, ChannelsSection, TokensSection],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="page">
      <header class="page-header">
        <div>
          <h1>Settings</h1>
          <p class="subtitle">How this window behaves, the service, and the defaults every runner uses.</p>
        </div>
      </header>
      <div class="layout">
        <nav class="sub" aria-label="Settings sections">
          @for (s of sections; track s.key) {
            <button type="button" [attr.aria-current]="current() === s.key ? 'page' : null" (click)="go(s.key)">
              <mat-icon [svgIcon]="'pc:' + s.icon" aria-hidden="true" />{{ s.label }}
            </button>
          }
        </nav>
        <div class="content">
          @switch (current()) {
            @case ('general') {
              <pc-general-section />
            }
            @case ('service') {
              <pc-service-section />
            }
            @case ('defaults') {
              <pc-defaults-section />
            }
            @case ('channels') {
              <pc-channels-section />
            }
            @case ('tokens') {
              <pc-tokens-section />
            }
            @case ('diagnostics') {
              <section class="card">
                <header class="card-header"><h2>Diagnostics</h2></header>
                <div class="card-body body">
                  <p class="muted">
                    Writes a zip with Pitcrew’s configuration, its own logs, the service state and the last lines of each runner’s log, for a bug report.
                    Tokens and registration files are never included, and secrets in log lines are masked.
                  </p>
                  <div class="btn-row">
                    <button type="button" class="btn primary" (click)="exportDiagnostics()" [disabled]="exporting()">
                      <mat-icon svgIcon="pc:package" aria-hidden="true" />{{ exporting() ? 'Writing…' : 'Export diagnostics' }}
                    </button>
                  </div>
                  @if (exported(); as path) {
                    <div class="callout success" role="status">
                      <mat-icon svgIcon="pc:check-circle" aria-hidden="true" />
                      <div class="callout-body">
                        <strong>Saved</strong>
                        <span class="mono path">{{ path }}</span>
                      </div>
                    </div>
                  }
                  @if (exportError(); as e) {
                    <p class="tone-danger" role="alert">{{ e }}</p>
                  }
                </div>
              </section>
            }
            @case ('about') {
              <section class="card about">
                <div class="card-body about-body">
                  <pc-brand-logo [size]="72" />
                  <div>
                    <h2>Pitcrew</h2>
                    <p class="muted">A runner manager for Gitea and Forgejo: keeps runners alive, updates them between jobs, cleans their disks and shows their state.</p>
                    <dl class="kv">
                      <dt>App</dt>
                      <dd class="num">{{ app()?.currentVersion ?? '—' }}</dd>
                      <dt>Service</dt>
                      <dd class="num">{{ status.service()?.version ?? 'not connected' }}</dd>
                      <dt>Licence</dt>
                      <dd>MIT or Apache-2.0</dd>
                    </dl>
                    <p class="faint small">“Gitea” and “Forgejo” are trademarks of their respective owners. Pitcrew is independent and not affiliated with either.</p>
                  </div>
                </div>
              </section>
            }
          }
        </div>
      </div>
    </div>
  `,
  styles: `
    .layout {
      display: grid;
      grid-template-columns: 190px 1fr;
      gap: var(--space-6);
      align-items: start;
    }
    .sub {
      position: sticky;
      top: var(--space-6);
      display: grid;
      gap: 2px;
    }
    .sub button {
      display: flex;
      align-items: center;
      gap: var(--space-2);
      height: 32px;
      padding: 0 var(--space-2);
      border: 0;
      border-radius: var(--radius-sm);
      background: none;
      color: var(--text-2);
      font: inherit;
      font-weight: var(--weight-medium);
      text-align: left;
      cursor: pointer;
    }
    .sub button:hover {
      background: var(--hover);
      color: var(--text);
    }
    .sub button[aria-current='page'] {
      background: var(--active);
      color: var(--text);
    }
    .sub mat-icon {
      width: 16px;
      height: 16px;
    }
    .content {
      display: grid;
      gap: var(--space-4);
      min-width: 0;
      max-width: 880px;
    }
    h2 {
      font-size: var(--text-md);
    }
    .body {
      display: grid;
      gap: var(--space-3);
    }
    .path {
      overflow-wrap: anywhere;
    }
    .about-body {
      display: flex;
      gap: var(--space-6);
      align-items: flex-start;
      padding: var(--space-6);
    }
    .about-body > div {
      display: grid;
      gap: var(--space-3);
    }
    .about h2 {
      font-family: var(--font-display);
      font-size: var(--text-xl);
    }
    .small {
      font-size: var(--text-sm);
    }
  `,
})
export class SettingsPage {
  /** Query parameter. */
  readonly section = input<string | undefined>(undefined);

  protected readonly status = inject(StatusStore);
  private readonly backend = inject(BACKEND);
  private readonly router = inject(Router);

  protected readonly sections = SECTIONS;
  protected readonly current = computed<SettingsSection>(() => (SECTIONS.some((s) => s.key === this.section()) ? (this.section() as SettingsSection) : 'general'));
  protected readonly exporting = signal(false);
  protected readonly exported = signal<string | null>(null);
  protected readonly exportError = signal<string | null>(null);
  protected readonly app = signal<AppUpdateStatus | null>(null);

  constructor() {
    this.backend.appUpdateStatus().then(
      (a) => this.app.set(a),
      () => undefined,
    );
  }

  protected go(section: SettingsSection): void {
    void this.router.navigate([], { queryParams: { section: section === 'general' ? null : section }, replaceUrl: true });
  }

  protected async exportDiagnostics(): Promise<void> {
    this.exporting.set(true);
    this.exportError.set(null);
    try {
      this.exported.set(await this.backend.exportDiagnostics());
    } catch (e) {
      this.exportError.set(toCommandError(e).message);
    } finally {
      this.exporting.set(false);
    }
  }
}
