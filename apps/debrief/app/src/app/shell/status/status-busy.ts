import { ChangeDetectionStrategy, Component, computed, inject } from '@angular/core';
import { WorkspaceStatusStore } from '../../core/workspace/status.store';

/** Status item (end): work other than an import (its own item shows imports), with a spinner. */
@Component({
  selector: 'debrief-status-busy',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @if (otherBusy(); as busy) {
      <span class="busy keep" role="status" data-testid="status-busy"><span class="spinner" aria-hidden="true"></span>{{ busy }}</span>
    }
  `,
  styles: `
    :host { display: contents; }
    .keep { flex: none; }
    .busy { display: inline-flex; align-items: center; gap: 6px; }
    .spinner { width: 9px; height: 9px; border-radius: 50%; border: 2px solid var(--border-3); border-top-color: var(--accent); animation: spin 0.8s linear infinite; }
    @keyframes spin { to { transform: rotate(360deg); } }
    @media (prefers-reduced-motion: reduce) { .spinner { animation: none; } }
  `,
})
export class StatusBusy {
  private readonly status = inject(WorkspaceStatusStore);
  /** "Applying the profile…", "Clearing workspace…". */
  protected readonly otherBusy = computed(() => (this.status.importing() ? null : this.status.busyLabel()));
}
