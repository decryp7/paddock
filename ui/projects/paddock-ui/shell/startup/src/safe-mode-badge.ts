import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { RecoveryStore } from './recovery.store';

/**
 * The "Safe mode" chip at the end of the title bar while the app runs in safe mode (plan X3): extensions, plugins and the
 * last session are off, so the user knows why things are missing. Draws nothing in a normal run. Needs a `RecoveryStore`.
 */
@Component({
  selector: 'pd-safe-mode-badge',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @if (store.safeMode()) {
      <span class="badge" role="status" data-testid="safe-mode-badge" [attr.title]="t.title">
        <svg viewBox="0 0 16 16" width="12" height="12" aria-hidden="true" focusable="false">
          <path d="M8 1.75 13.5 4v4c0 3-2.3 5.1-5.5 6.25C4.8 13.1 2.5 11 2.5 8V4L8 1.75Z" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linejoin="round" />
        </svg>
        {{ t.label }}
      </span>
    }
  `,
  styles: `
    :host { display: inline-flex; min-width: 0; }
    .badge { display: inline-flex; align-items: center; gap: var(--pd-space-1); padding: 1px var(--pd-space-2); border: 1px solid var(--pd-warning); border-radius: var(--pd-radius-pill); color: var(--pd-text); font-size: var(--pd-text-caption); font-weight: var(--pd-weight-medium); white-space: nowrap; }
    svg { color: var(--pd-warning); }
  `,
})
export class PdSafeModeBadge {
  protected readonly store = inject(RecoveryStore);
  protected readonly t = {
    label: $localize`:@@pd.startup.safe-mode-label:Safe mode`,
    title: $localize`:@@pd.startup.safe-mode-title:Safe mode: extensions, plugins and the last session are off. Restart the app to leave it.`,
  };
}
