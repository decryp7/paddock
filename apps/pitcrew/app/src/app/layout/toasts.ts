import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { MatIcon } from '@angular/material/icon';
import { ToastStore } from '../state/toast.store';

/** The toast stack (announced politely to screen readers). */
@Component({
  selector: 'pc-toasts',
  imports: [MatIcon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="stack" aria-live="polite" aria-relevant="additions">
      @for (t of store.toasts(); track t.id) {
        <div class="toast" [class]="t.tone" role="status">
          <mat-icon [svgIcon]="'pc:' + t.icon" aria-hidden="true" />
          <div class="body">
            <strong>{{ t.title }}</strong>
            @if (t.body) {
              <span>{{ t.body }}</span>
            }
          </div>
          <button type="button" class="btn ghost icon sm" (click)="store.dismiss(t.id)" aria-label="Dismiss">
            <mat-icon svgIcon="pc:x" aria-hidden="true" />
          </button>
        </div>
      }
    </div>
  `,
  styles: `
    .stack {
      position: fixed;
      right: var(--space-5);
      bottom: var(--space-5);
      z-index: var(--z-toast);
      display: grid;
      gap: var(--space-2);
      width: min(380px, calc(100vw - 40px));
    }
    .toast {
      display: flex;
      gap: var(--space-3);
      align-items: flex-start;
      padding: var(--space-3);
      border: 1px solid var(--border-strong);
      border-radius: var(--radius-md);
      background: var(--surface);
      box-shadow: var(--shadow-lg);
      animation: pc-fade-in var(--duration-slow) var(--ease-out);
    }
    .toast > mat-icon {
      flex: none;
      width: 18px;
      height: 18px;
      margin-top: 1px;
      color: var(--neutral);
    }
    .success > mat-icon {
      color: var(--success);
    }
    .info > mat-icon {
      color: var(--info);
    }
    .warning > mat-icon {
      color: var(--warning);
    }
    .danger > mat-icon {
      color: var(--danger);
    }
    .body {
      flex: 1;
      display: grid;
      gap: 2px;
      min-width: 0;
    }
    .body span {
      color: var(--text-2);
      font-size: var(--text-sm);
    }
  `,
})
export class Toasts {
  protected readonly store = inject(ToastStore);
}
