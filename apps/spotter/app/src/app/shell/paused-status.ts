import { ChangeDetectionStrategy, Component, computed, inject } from '@angular/core';
import { ClockService } from '../core/clock.service';
import { spanText } from '../shared/format';
import { SettingsStore } from '../state/settings.store';

/** Are notifications paused at `nowMs` (a pause that has run out is over, whether or not the engine has cleared it yet)? */
export function isPaused(untilMs: number | null | undefined, nowMs: number): boolean {
  return untilMs !== null && untilMs !== undefined && untilMs > nowMs;
}

/** What the status bar says while notifications are paused: "Paused for 42 min" (the time left, rounded up to a minute). */
export function pausedText(untilMs: number, nowMs: number): string {
  return `Notifications paused for ${spanText(Math.max(untilMs - nowMs, 60_000))}`;
}

/** The status item of paused notifications (shown by `notificationsPaused`; Resume is in the File menu and the palette). */
@Component({
  selector: 'sp-paused-status',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `<span class="text" data-testid="paused-status">{{ text() }}</span>`,
  styles: `:host { display: inline-flex; min-width: 0; } .text { white-space: nowrap; }`,
})
export class PausedStatus {
  private readonly settings = inject(SettingsStore);
  private readonly clock = inject(ClockService);
  protected readonly text = computed(() => {
    const until = this.settings.settings().value?.notificationsPausedUntilMs ?? null;
    return until === null ? '' : pausedText(until, this.clock.now());
  });
}
