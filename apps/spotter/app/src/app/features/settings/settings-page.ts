import { ChangeDetectionStrategy, Component, computed, inject } from '@angular/core';
import { MatButtonToggle, MatButtonToggleGroup } from '@angular/material/button-toggle';
import { MatIcon } from '@angular/material/icon';
import { MatProgressBar } from '@angular/material/progress-bar';
import { MatSlideToggle } from '@angular/material/slide-toggle';
import { jobView } from '@decryp7/paddock-ui';
import { JobsStore } from '@decryp7/paddock-ui/shell/jobs';
import type { Settings } from '../../core/api.types';
import { ClockService } from '../../core/clock.service';
import { formatDateTime, spanText } from '../../shared/format';
import { Panel } from '../../shared/ui/panel';
import { MailSettings } from './mail-settings';
import { SettingsStore } from '../../state/settings.store';

/** Spotter's settings page (a page of the shell's Settings window): startup, notifications, checks, downloads, the page renderer, mail and data. The shell adds Appearance and Updates. */
@Component({
  selector: 'sp-settings-page',
  imports: [MatIcon, MatSlideToggle, MatButtonToggleGroup, MatButtonToggle, MatProgressBar, Panel, MailSettings],
  changeDetection: ChangeDetectionStrategy.OnPush,
  templateUrl: './settings-page.html',
  styleUrl: './settings-page.scss',
})
export class SettingsPage {
  protected readonly store = inject(SettingsStore);
  private readonly clock = inject(ClockService);
  /** The window's jobs (the Settings window provides them; a host without them just shows no progress). */
  private readonly jobs = inject(JobsStore, { optional: true });
  protected readonly intervals = [
    { secs: 900, label: '15 minutes' },
    { secs: 3600, label: '1 hour' },
    { secs: 6 * 3600, label: '6 hours' },
    { secs: 24 * 3600, label: 'Daily' },
  ];

  protected readonly s = computed(() => this.store.settings().value);
  protected readonly pausedFor = computed(() => {
    const until = this.s()?.notificationsPausedUntilMs ?? null;
    return until !== null && until > this.clock.now() ? spanText(until - this.clock.now()) : null;
  });
  /** Which pause button is lit: On, or the nearest of the two pauses to what is left. */
  protected readonly pauseValue = computed(() => {
    const until = this.s()?.notificationsPausedUntilMs ?? null;
    if (until === null || until <= this.clock.now()) return 'on';
    return until - this.clock.now() <= 3_600_000 ? '1h' : '1d';
  });
  /** The browser install while it runs: its phase and count in words, from the shell's job (the engine owns it). */
  private readonly installJob = computed(() => this.jobs?.active().find((j) => j.kind === 'sp.install-renderer') ?? null);
  protected readonly install = computed(() => {
    const j = this.installJob();
    return j ? jobView(j) : null;
  });
  protected readonly rendererTone = computed(() => {
    const k = this.store.renderer().value?.kind;
    return k === 'chromium-installed' || k === 'chromium-downloaded' ? 'success' : k === 'os-webview' ? 'warning' : 'neutral';
  });
  protected readonly rendererWord = computed(() => {
    switch (this.store.renderer().value?.kind) {
      case 'chromium-installed':
        return 'Chromium found';
      case 'chromium-downloaded':
        return 'Chromium installed by Spotter';
      case 'os-webview':
        return 'OS web view (limited)';
      default:
        return 'No browser available';
    }
  });
  protected readonly dateTime = formatDateTime;

  constructor() {
    void this.store.loadAll();
  }

  protected set(patch: Partial<Settings>): void {
    void this.store.change(patch);
  }

  protected value(e: Event): string {
    return (e.target as HTMLInputElement | HTMLSelectElement).value;
  }

  protected onPause(value: 'on' | '1h' | '1d'): void {
    void this.store.pauseNotifications(value === 'on' ? null : value === '1h' ? 60 : 24 * 60);
  }

  protected cancelInstall(): void {
    const j = this.installJob();
    if (j) void this.jobs?.cancel(j.id);
  }

  protected setQuiet(on: boolean): void {
    this.set({ quietHours: on ? { start: '22:00', end: '07:00' } : null });
  }

  protected setQuietTime(which: 'start' | 'end', value: string): void {
    const q = this.s()?.quietHours;
    if (q) this.set({ quietHours: { ...q, [which]: value } });
  }
}
