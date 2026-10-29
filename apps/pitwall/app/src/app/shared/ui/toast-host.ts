import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { MatIconModule } from '@angular/material/icon';
import { Toasts } from '../../core/toasts';

/** Renders {@link Toasts} bottom-right in a polite live region so screen readers hear them. */
@Component({
  selector: 'pw-toast-host',
  imports: [MatIconModule],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="stack" role="status" aria-live="polite">
      @for (t of toasts.toasts(); track t.id) {
        <div class="toast" [attr.data-tone]="t.tone">
          <mat-icon [svgIcon]="t.tone === 'bad' ? 'pw:error' : t.tone === 'ok' ? 'pw:success' : 'pw:info'" aria-hidden="true" />
          <span>{{ t.message }}</span>
          <button type="button" class="btn ghost icon sm" aria-label="Dismiss" (click)="toasts.dismiss(t.id)"><mat-icon svgIcon="pw:close" aria-hidden="true" /></button>
        </div>
      }
    </div>
  `,
  styles: `
    .stack {
      position: fixed;
      right: var(--pd-space-4);
      bottom: var(--pd-space-4);
      z-index: 1100;
      display: flex;
      flex-direction: column;
      gap: var(--pd-space-2);
      pointer-events: none;
    }
    .toast {
      display: flex;
      align-items: center;
      gap: var(--pd-space-2);
      max-width: 420px;
      padding: var(--pd-space-2) var(--pd-space-2) var(--pd-space-2) var(--pd-space-3);
      font-size: var(--pd-text-body);
      color: var(--pd-text);
      background: var(--pd-surface-3);
      border: 1px solid var(--pd-hairline);
      border-radius: var(--pd-radius-card);
      box-shadow: var(--pd-shadow-3);
      pointer-events: auto;
      animation: pw-pop var(--pd-duration) var(--pd-ease-move);
      mat-icon {
        flex: none;
      }
      span {
        flex: 1;
      }
      &[data-tone='ok'] > mat-icon {
        color: var(--pd-success);
      }
      &[data-tone='bad'] > mat-icon {
        color: var(--pd-danger);
      }
      &[data-tone='info'] > mat-icon {
        color: var(--pd-accent);
      }
    }
  `,
})
export class ToastHost {
  protected readonly toasts = inject(Toasts);
}
