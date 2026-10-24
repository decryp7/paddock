import { ChangeDetectionStrategy, Component } from '@angular/core';
import { DemoShortcuts } from '../demo-shortcuts';
import { DemoPage } from './demo-page';

/** The keyboard shortcuts (plan S13): every command with its key, changeable and kept. */
@Component({
  selector: 'sd-keys-page',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [DemoPage, DemoShortcuts],
  template: `
    <sd-page heading="Keyboard" subtitle="The menu bar and the keys come from the contributions; a changed key is kept, a taken one is refused.">
      <sd-shortcuts />
    </sd-page>
  `,
})
export class KeysPage {}
