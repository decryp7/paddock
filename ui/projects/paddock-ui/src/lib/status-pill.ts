import { ChangeDetectionStrategy, Component, input } from '@angular/core';
import { SU_STATUS_ICON, type SuStatus } from './status';

/** Badge: tint background, status-coloured text, a 12 px icon and a word. Never colour alone. */
@Component({
  selector: 'pd-status-pill',
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: { '[class]': '"pd-pill pd-pill--" + status()' },
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
      gap: var(--pd-space-1);
      height: 20px;
      padding: 0 var(--pd-space-2);
      border-radius: var(--pd-radius-pill);
      font-size: var(--pd-text-label);
      line-height: var(--pd-lh-label);
      font-weight: var(--pd-weight-medium);
      white-space: nowrap;
      background: var(--pd-neutral-tint);
      color: var(--pd-neutral);
    }
    :host(.pd-pill--success) { background: var(--pd-success-tint); color: var(--pd-success); }
    :host(.pd-pill--warning) { background: var(--pd-warning-tint); color: var(--pd-warning); }
    :host(.pd-pill--danger) { background: var(--pd-danger-tint); color: var(--pd-danger); }
    :host(.pd-pill--info) { background: var(--pd-info-tint); color: var(--pd-info); }
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
