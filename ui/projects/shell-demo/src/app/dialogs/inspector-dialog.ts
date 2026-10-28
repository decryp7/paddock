import { ChangeDetectionStrategy, Component, computed, inject, resource } from '@angular/core';
import { formatDuration } from '@decryp7/paddock-ui/format';
import type { DialogAction, DialogContent } from '@decryp7/paddock-ui/shell/dialogs';
import { DemoEngine } from './demo-ipc';

/** A form: modeless, medium, fitted to its three facts. It reads the engine through its own providers (`demo_status`). */
@Component({
  selector: 'sd-inspector-dialog',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <section class="text" aria-label="Engine status">
      @switch (status.status()) {
        @case ('resolved') {
          <dl class="facts" data-testid="engine-status">
            <dt>Engine</dt><dd data-testid="engine-state">{{ state(status.value()?.engine) }}</dd>
            <dt>Up for</dt><dd>{{ uptime(status.value()?.uptimeSeconds) }}</dd>
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

  /** The engine's state as a word at the start of a value: "Running". */
  protected state(engine: string | undefined): string {
    return engine === undefined ? '' : engine.charAt(0).toUpperCase() + engine.slice(1);
  }

  /** "12 min 34 s": the engine's seconds as a person reads them. */
  protected uptime(seconds: number | undefined): string {
    return formatDuration(seconds === undefined ? null : seconds * 1000);
  }
}
