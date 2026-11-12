import { ChangeDetectionStrategy, Component, inject, signal } from '@angular/core';
import { ContributionRegistry, type DialogDef } from '@decryp7/paddock-ui/shell/contributions';
import { DialogService } from '@decryp7/paddock-ui/shell/dialogs';
import type { ConfirmInput } from '@decryp7/paddock-ui/shell/confirm';
import { DemoPage, SECTION_STYLES } from './demo-page';

/** What the Dialogs page asks with the shared confirmation. */
const CONFIRM_SAMPLE: ConfirmInput = {
  heading: 'Forget the greetings?',
  message: 'The greeting count and the log go back to empty.',
  details: ['3 greetings', 'The greeting log'],
  confirm: { label: 'Forget greetings', destructive: true },
};

/** One dialog of each kind (plan S6): OS windows in the desktop build, in the page in the browser build. */
@Component({
  selector: 'sd-dialogs-page',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [DemoPage],
  template: `
    <sd-page heading="Dialogs" subtitle="One of each kind. In the desktop build each is an OS window; in the browser build they open in the page.">
      <section aria-label="Dialogs">
        <div class="commands" role="group" aria-label="Open a dialog">
          @for (d of dialogs; track d.id) {
            <button type="button" class="cmd" [attr.data-dialog-open]="d.id" (click)="open(d)">
              {{ d.title.text }} <span class="kind">({{ d.kind }}{{ d.requiredChoice ? ', required choice' : '' }})</span>
            </button>
          }
        </div>
        <p class="context" aria-live="polite" data-testid="dialog-answer">{{ answer() }}</p>
      </section>
    </sd-page>
  `,
  styles: SECTION_STYLES + `.kind { color: var(--pd-text-muted); font-weight: var(--pd-weight-regular); }`,
})
export class DialogsPage {
  private readonly dialogService = inject(DialogService);
  protected readonly dialogs = inject(ContributionRegistry).dialogs();
  protected readonly answer = signal('No dialog answered yet.');

  protected open(d: DialogDef): void {
    const inputs: Readonly<Record<string, unknown>> = d.id === 'sd.rename' ? { current: 'Shell demo' } : d.id === 'pd.confirm' ? { ...CONFIRM_SAMPLE } : {};
    void this.dialogService.open(d.id, { inputs }).closed.then((o) => this.answer.set(`${d.title.text}: ${o.kind === 'dismissed' ? 'dismissed' : JSON.stringify(o.value ?? null)}`));
  }
}
