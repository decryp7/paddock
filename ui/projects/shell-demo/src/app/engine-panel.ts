import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { PdServiceError } from '@decryp7/paddock-ui/shell/notify';
import { DemoEngineData } from './demo-health';

/** A panel that needs the demo engine: it waits calmly while the engine is down and loads again by itself after Repair. */
@Component({
  selector: 'sd-engine-panel',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [PdServiceError],
  template: `
    <section class="panel" aria-labelledby="sd-engine-data">
      <h2 id="sd-engine-data">Engine data</h2>
      @if (data.failed()) {
        <pd-service-error summary="Could not load the engine data." subject="Engine data" service="sd.engine" (retry)="data.load()" />
      } @else {
        <p data-testid="engine-data">Engine data loaded ({{ data.loads() }} {{ data.loads() === 1 ? 'load' : 'loads' }})</p>
      }
    </section>
  `,
  styles: `
    .panel { display: flex; flex-direction: column; gap: var(--pd-space-2); padding: var(--pd-space-3); border: 1px solid var(--pd-hairline); border-radius: var(--pd-radius-card); background: var(--pd-surface-1); }
    h2 { margin: 0; font-size: var(--pd-text-subtitle); }
    p { margin: 0; }
  `,
})
export class EnginePanel {
  protected readonly data = inject(DemoEngineData);

  constructor() {
    this.data.load();
  }
}
