import { ChangeDetectionStrategy, Component, computed, inject } from '@angular/core';
import { PdPathText } from '@decryp7/paddock-ui';
import { VIEW_PANEL } from '@decryp7/paddock-ui/workbench';
import { DemoRepo } from '../repo';

/** A read-only code view (a document with instances: one panel per file, `wd.code#<path>`). Text is bound as text. */
@Component({
  selector: 'wd-code-view',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [PdPathText],
  template: `
    <header class="head"><pd-path-text [value]="path()" [copy]="true" /></header>
    @if (file(); as f) {
      <ol class="code" [attr.aria-label]="path()" data-testid="code-lines">
        @for (line of lines(); track $index) {
          <li><code>{{ line }}</code></li>
        }
      </ol>
    } @else {
      <p class="none">Choose a file in the Explorer or press Ctrl+P to go to one.</p>
    }
  `,
  styles: `
    :host { display: flex; flex-direction: column; height: 100%; min-width: 0; overflow: hidden; background: var(--pd-bg); }
    .head { flex: none; padding: var(--pd-space-1) var(--pd-pane-inset, var(--pd-space-3)); border-bottom: 1px solid var(--pd-hairline); min-width: 0; }
    .code { flex: 1; margin: 0; padding: var(--pd-space-2) var(--pd-pane-inset, var(--pd-space-3)); list-style: none; overflow: auto; font-family: var(--pd-font-mono); font-size: var(--pd-text-label); line-height: 1.6; counter-reset: none; }
    .code li { padding-inline-start: 0; white-space: pre; color: var(--pd-text); }
    .code li::marker { color: var(--pd-text-subtle); }
    .none { padding: var(--pd-space-6); color: var(--pd-text-muted); }
  `,
})
export class CodeView {
  private readonly panel = inject(VIEW_PANEL, { optional: true });
  private readonly repo = inject(DemoRepo);
  protected readonly path = computed(() => this.panel?.key ?? 'README.md');
  protected readonly file = computed(() => this.repo.file(this.path()));
  protected readonly lines = computed(() => (this.file()?.text ?? '').replace(/\n$/, '').split('\n'));
}
