import { ChangeDetectionStrategy, Component, computed, inject, resource } from '@angular/core';
import type { DialogAction, DialogContent } from '@decryp7/paddock-ui/shell/dialogs';
import { DemoEngine } from './demo-ipc';

/** A tool: modeless, user-sized, remembers its size. It reads the engine through its own providers (`demo_status`). */
@Component({
  selector: 'sd-inspector-dialog',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <section class="text" aria-label="Engine status">
      @switch (status.status()) {
        @case ('resolved') {
          <dl class="facts" data-testid="engine-status">
            <dt>Engine</dt><dd>{{ status.value()?.engine }}</dd>
            <dt>Up for</dt><dd>{{ status.value()?.uptimeSeconds }} s</dd>
            <dt>Queued</dt><dd>{{ status.value()?.queued }}</dd>
          </dl>
        }
        @case ('error') {
          <p role="alert">The engine did not answer.</p>
        }
        @default {
          <p>Asking the engine…</p>
        }
      }
    </section>
  `,
  styleUrl: './dialog-content.css',
})
export class InspectorDialog implements DialogContent {
  private readonly engine = inject(DemoEngine);
  protected readonly status = resource({ loader: () => this.engine.status() });
  /** Refresh at the leading edge; the frame adds Close. */
  readonly dialogActions = computed<readonly DialogAction[]>(() => [{ kind: 'run', id: 'refresh', label: 'Refresh', role: 'secondary', lead: true, run: () => void this.status.reload() }]);
  readonly dialogStatus = computed(() => (this.status.status() === 'resolved' ? `Queued: ${this.status.value()?.queued ?? 0}` : null));
}
