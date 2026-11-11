import { ChangeDetectionStrategy, Component, computed, inject } from '@angular/core';
import { LiveTail } from '../../workspace/live-tail';

/** Status item (end): the Live tail toggle. */
@Component({
  selector: 'debrief-status-live',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <button
      type="button"
      class="live"
      data-testid="live-toggle"
      aria-label="Live tail"
      i18n-aria-label="@@shell.statusBar.liveTail"
      [class.on]="live.enabled()"
      [class.failed]="!!live.error()"
      [attr.aria-pressed]="live.enabled()"
      [disabled]="!live.available()"
      [title]="liveTitle()"
      (click)="live.toggle()"
      i18n="@@shell.statusBar.live"
    >
      <span class="live-dot" aria-hidden="true"></span>Live
    </button>
  `,
  styles: `
    :host { display: contents; }
    .live { display: inline-flex; align-items: center; gap: 6px; height: 18px; padding: 0 8px; border-radius: 9px; border: 1px solid currentColor; background: transparent; color: inherit; cursor: pointer; font-size: var(--fs-xs); font-weight: 600; }
    .live:hover:not(:disabled) { background: var(--bg-hover); color: var(--statusbar-strong); }
    .live:disabled { cursor: default; opacity: 0.5; }
    .live-dot { width: 7px; height: 7px; border-radius: 50%; border: 1px solid currentColor; }
    .live.on, .live.on:hover { background: var(--accent); color: var(--accent-ink); border-color: var(--accent); }
    .live.on .live-dot { background: currentColor; animation: live-pulse 1.6s ease-in-out infinite; }
    .live.failed { border-style: dashed; }
    @keyframes live-pulse { 50% { opacity: 0.35; } }
    @media (prefers-reduced-motion: reduce) { .live.on .live-dot { animation: none; } }
  `,
})
export class StatusLive {
  protected readonly live = inject(LiveTail);
  protected readonly liveTitle = computed(() => {
    const error = this.live.error();
    if (error) return $localize`:@@shell.statusBar.liveFailed:Live tail failed: ${error}:error:`;
    if (this.live.enabled()) return $localize`:@@shell.statusBar.liveOn:Live tail is on: new lines appear as the files grow. Click to stop.`;
    return this.live.available()
      ? $localize`:@@shell.statusBar.liveFollow:Follow the opened files as they grow`
      : $localize`:@@shell.statusBar.liveOpenFiles:Open files to follow them live`;
  });
}
