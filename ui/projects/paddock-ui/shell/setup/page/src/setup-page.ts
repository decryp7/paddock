import { ChangeDetectionStrategy, Component } from '@angular/core';
import { PdSetupChecklist } from './setup-checklist';

/** Settings > Set-up: the way back to the checklist after it was shown or finished (plan X19). */
@Component({
  selector: 'pd-setup-page',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [PdSetupChecklist],
  template: `
    <section aria-labelledby="pd-setup-title" data-testid="setup-page">
      <h2 id="pd-setup-title">{{ title }}</h2>
    </section>
    <pd-setup-checklist />
  `,
  styles: `
    :host { display: flex; flex-direction: column; gap: var(--pd-space-4); min-width: 0; color: var(--pd-text); }
    h2 { margin: 0; font-size: var(--pd-text-subtitle); font-weight: var(--pd-weight-semibold); }
  `,
})
export class PdSetupPage {
  protected readonly title = $localize`:@@pd.setup.title:Set-up`;
}
