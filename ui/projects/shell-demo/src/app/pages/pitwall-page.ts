import { ChangeDetectionStrategy, Component, inject, signal } from '@angular/core';
import { PdNeedsAi, PitwallStore } from '@decryp7/paddock-ui/shell/pitwall';
import { DemoPage, SECTION_STYLES } from './demo-page';
import { DEMO_PITWALL_STATES, DemoPitwall } from './pitwall.providers';

/** The Local AI page: the shared needs-AI state in front of a feature, and the demo's controls for Pitwall's side. */
@Component({
  selector: 'sd-pitwall-page',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [DemoPage, PdNeedsAi],
  template: `
    <sd-page heading="Local AI" subtitle="The shared “needs local AI” state of the shell, on a fake Pitwall.">
      <section aria-labelledby="sd-feature">
        <h2 id="sd-feature">Ask about this log</h2>
        @if (store.ready()) {
          <p class="context" data-testid="pitwall-answer">Pitwall answers this feature with {{ store.snapshot()?.state?.kind === 'ready' ? model() : '' }}.</p>
        } @else if (store.onBuiltIn()) {
          <p class="context" data-testid="pitwall-answer">This feature runs on the built-in engine for this session.</p>
        }
        <pd-needs-ai feature="Ask about this log" settingsLink (step)="last.set('step: ' + $event)" (openSettings)="last.set('settings opened')" />
        <p class="context" data-testid="pitwall-last" aria-live="polite">{{ last() || 'Nothing chosen yet.' }}</p>
      </section>
      <section aria-labelledby="sd-pitwall-side">
        <h2 id="sd-pitwall-side">Pitwall’s side (demo)</h2>
        <div class="commands" role="group" aria-labelledby="sd-pitwall-side">
          @for (name of names; track name) {
            <button type="button" class="cmd" [attr.data-testid]="'pitwall-state-' + name" (click)="set(name)">{{ name }}</button>
          }
          <button type="button" class="cmd" data-testid="pitwall-no-engine" (click)="demo.engine.setBuiltInAvailable(false)">No engine of its own</button>
          <button type="button" class="cmd" data-testid="pitwall-approve" (click)="demo.engine.approve()">Approve in Pitwall</button>
        </div>
      </section>
    </sd-page>
  `,
  styles: SECTION_STYLES,
})
export class PitwallPage {
  protected readonly store = inject(PitwallStore);
  protected readonly demo = inject(DemoPitwall);
  protected readonly names = Object.keys(DEMO_PITWALL_STATES);
  protected readonly last = signal('');

  protected model(): string {
    const s = this.store.snapshot()?.state;
    return s?.kind === 'ready' ? s.model : '';
  }

  protected set(name: string): void {
    const state = DEMO_PITWALL_STATES[name];
    if (state) this.demo.engine.setState(state);
  }
}
