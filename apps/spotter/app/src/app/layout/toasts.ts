import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { MatIcon } from '@angular/material/icon';
import { ToastStore } from '../state/toast.store';

/** The toast stack (announced politely to screen readers). */
@Component({
  selector: 'sp-toasts',
  imports: [MatIcon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="stack" aria-live="polite" aria-relevant="additions">
      @for (t of store.toasts(); track t.id) {
        <div class="toast" [class]="t.tone" role="status">
          <mat-icon [svgIcon]="'sp:' + t.icon" aria-hidden="true" />
          <div class="body">
            <strong>{{ t.title }}</strong>
            @if (t.body) {
              <span class="pd-selectable">{{ t.body }}</span>
            }
          </div>
          <button type="button" class="btn ghost icon sm" (click)="store.dismiss(t.id)" aria-label="Dismiss">
            <mat-icon svgIcon="sp:x" aria-hidden="true" />
          </button>
        </div>
      }
    </div>
  `,
  styles: `
    .stack {
      position: fixed;
      right: var(--pd-space-4);
      bottom: var(--pd-space-4);
      z-index: var(--pd-z-toast);
      display: grid;
      gap: var(--pd-space-2);
      width: min(380px, calc(100vw - 32px));
    }
    .toast {
      display: flex;
      gap: var(--pd-space-3);
      align-items: flex-start;
      padding: var(--pd-space-3);
      border: 1px solid var(--pd-hairline);
      border-radius: var(--pd-radius-popover);
      background: var(--pd-surface-3);
      box-shadow: var(--pd-shadow-3);
    }
    .toast > mat-icon {
      flex: none;
      width: 18px;
      height: 18px;
      margin-top: 1px;
      color: var(--pd-neutral);
    }
    .success > mat-icon {
      color: var(--pd-success);
    }
    .info > mat-icon {
      color: var(--pd-info);
    }
    .warning > mat-icon {
      color: var(--pd-warning);
    }
    .danger > mat-icon {
      color: var(--pd-danger);
    }
    .body {
      flex: 1;
      display: grid;
      gap: 2px;
      min-width: 0;
    }
    .body span {
      color: var(--pd-text-muted);
      font-size: var(--pd-text-caption);
      overflow-wrap: anywhere;
    }
  `,
})
export class Toasts {
  protected readonly store = inject(ToastStore);
}
