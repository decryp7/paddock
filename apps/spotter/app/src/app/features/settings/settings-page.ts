import { ChangeDetectionStrategy, Component, computed, inject } from '@angular/core';
import { MatIcon } from '@angular/material/icon';
import { SuSegmentedControl, type SuSegment } from '@decryp7/paddock-ui';
import type { Settings, Theme } from '../../core/api.types';
import { ClockService } from '../../core/clock.service';
import { formatDateTime, spanText } from '../../shared/format';
import { Panel } from '../../shared/ui/panel';
import { MailSettings } from './mail-settings';
import { SettingsStore } from '../../state/settings.store';

const THEMES: SuSegment[] = [
  { value: 'system', label: 'System' },
  { value: 'light', label: 'Light' },
  { value: 'dark', label: 'Dark' },
];

/** Settings: appearance, notifications, checks, downloads, the page renderer, updates and data. */
@Component({
  selector: 'sp-settings-page',
  imports: [MatIcon, SuSegmentedControl, Panel, MailSettings],
  changeDetection: ChangeDetectionStrategy.OnPush,
  templateUrl: './settings-page.html',
  styleUrl: './settings-page.scss',
})
export class SettingsPage {
  protected readonly store = inject(SettingsStore);
  private readonly clock = inject(ClockService);
  protected readonly themes = THEMES;
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
  protected readonly rendererTone = computed(() => {
    const k = this.store.renderer().value?.kind;
    return k === 'chromium-installed' || k === 'chromium-downloaded' ? 'success' : k === 'os-webview' ? 'warning' : 'neutral';
  });
  protected readonly rendererWord = computed(() => {
    switch (this.store.renderer().value?.kind) {
      case 'chromium-installed':
        return 'Chromium found';
      case 'chromium-downloaded':
        return 'Chromium downloaded';
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

  protected setTheme(v: string): void {
    this.set({ theme: v as Theme });
  }

  protected checked(e: Event): boolean {
    return (e.target as HTMLInputElement).checked;
  }

  protected value(e: Event): string {
    return (e.target as HTMLInputElement | HTMLSelectElement).value;
  }

  protected setQuiet(on: boolean): void {
    this.set({ quietHours: on ? { start: '22:00', end: '07:00' } : null });
  }

  protected setQuietTime(which: 'start' | 'end', value: string): void {
    const q = this.s()?.quietHours;
    if (q) this.set({ quietHours: { ...q, [which]: value } });
  }
}
