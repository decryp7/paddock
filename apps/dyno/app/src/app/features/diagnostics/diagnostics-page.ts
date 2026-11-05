import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { SHELL_PORTS } from '@decryp7/paddock-ui/shell';
import { MatButton } from '@angular/material/button';
import { Icon } from '../../shared/icon';

const NEVER = [
  'The contents of your documents or any file outside Dyno',
  'Browser data: history, cookies, saved logins',
  'Passwords, keys and tokens (they are replaced where Dyno can recognise them)',
  'Command lines of the programs you run',
];

/**
 * The diagnostics page: one explanation and one button. There is one problem report (Help, Report a problem…); this page opens
 * the same dialog, which shows every file with its size before anything is saved. Nothing is sent anywhere.
 */
@Component({
  selector: 'dy-diagnostics-page',
  imports: [Icon, MatButton],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="page">
      <header class="page-head">
        <div class="titles">
          <h1>Diagnostics bundle</h1>
          <p>A zip for a bug report. Made only when you ask, saved where you choose, never sent anywhere.</p>
        </div>
        <button type="button" matButton="filled" data-testid="open-report" (click)="open()"><dy-icon name="file" />Report a problem…</button>
      </header>
      <section class="card" aria-label="Before you share the zip">
        <div class="card-head"><h2>Issue reports are public</h2></div>
        <p class="look">Look through the zip before you attach it. The dialog lists every file with its size, what was replaced, and what may remain.</p>
        <ul class="never" aria-label="What is never included">
          @for (n of never; track n) {
            <li><dy-icon name="close" />{{ n }}</li>
          }
        </ul>
        <p class="look">File names outside your home folder may remain.</p>
      </section>
    </div>
  `,
  styles: `
    :host { display: block; }
    .page { display: grid; gap: var(--sp-4); align-content: start; }
    .page-head .mat-mdc-button-base { align-self: start; }
    .never { list-style: none; margin: 0; padding: var(--sp-2) var(--sp-4) var(--sp-3); display: grid; }
    .never li { display: flex; gap: var(--sp-2); align-items: flex-start; padding: var(--sp-1) 0; color: var(--text-2); font-size: var(--fs-sm); }
    .never dy-icon { --icon-size: 13px; color: var(--text-3); margin-top: 2px; flex: none; }
    .look { margin: 0; padding: var(--sp-3) var(--sp-4); color: var(--text-2); font-size: var(--fs-sm); }
  `,
})
export class DiagnosticsPage {
  private readonly ports = inject(SHELL_PORTS);
  protected readonly never = NEVER;

  protected open(): void {
    void this.ports.openDialog('pd.report-problem');
  }
}
