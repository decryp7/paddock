import { ChangeDetectionStrategy, Component, inject, resource } from '@angular/core';
import { DIALOG_REF } from '@decryp7/paddock-ui/shell/dialogs';
import { DemoEngine } from './demo-ipc';

/** A tool: modeless, large, remembers its size. It reads the engine through its own providers (`demo_status`). */
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
    <div class="actions">
      <button type="button" class="btn" (click)="status.reload()">Refresh</button>
      <button type="button" class="btn" data-dialog-close (click)="ref.close()">Close</button>
    </div>
  `,
  styleUrl: './dialog-content.css',
})
export class InspectorDialog {
  private readonly engine = inject(DemoEngine);
  protected readonly ref = inject(DIALOG_REF);
  protected readonly status = resource({ loader: () => this.engine.status() });
}
