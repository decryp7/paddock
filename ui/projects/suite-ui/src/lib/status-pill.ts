import { ChangeDetectionStrategy, Component, input } from '@angular/core';
import { SU_STATUS_ICON, type SuStatus } from './status';

/** Badge: tint background, status-coloured text, a 12 px icon and a word. Never colour alone. */
@Component({
  selector: 'su-status-pill',
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: { '[class]': '"su-pill su-pill--" + status()' },
  template: `
    <svg class="icon" viewBox="0 0 16 16" width="12" height="12" aria-hidden="true" focusable="false">
      <path [attr.d]="iconPath()" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" />
    </svg>
    <span class="label">{{ label() }}</span>
  `,
  styles: `
    :host {
      display: inline-flex;
      align-items: center;
      gap: var(--su-space-1);
      height: 20px;
      padding: 0 var(--su-space-2);
      border-radius: var(--su-radius-pill);
      font-size: var(--su-text-label);
      line-height: var(--su-lh-label);
      font-weight: var(--su-weight-medium);
      white-space: nowrap;
      background: var(--su-neutral-tint);
      color: var(--su-neutral);
    }
    :host(.su-pill--success) { background: var(--su-success-tint); color: var(--su-success); }
    :host(.su-pill--warning) { background: var(--su-warning-tint); color: var(--su-warning); }
    :host(.su-pill--danger) { background: var(--su-danger-tint); color: var(--su-danger); }
    :host(.su-pill--info) { background: var(--su-info-tint); color: var(--su-info); }
    .icon { flex: none; }
  `,
})
export class SuStatusPill {
  /** The status role: decides colour and icon shape. */
  readonly status = input<SuStatus>('neutral');
  /** The word shown next to the icon (required for accessibility; status is never colour alone). */
  readonly label = input.required<string>();

  protected iconPath(): string {
    return SU_STATUS_ICON[this.status()];
  }
}
