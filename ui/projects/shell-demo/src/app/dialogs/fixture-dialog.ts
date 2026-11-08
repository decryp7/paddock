import { ChangeDetectionStrategy, Component, computed, signal } from '@angular/core';
import { DialogActionError, type DialogAction, type DialogContent } from '@decryp7/paddock-ui/shell/dialogs';

/**
 * The fixture of the size-to-content checks (ADR 0043 check 4), the same for every kind: a button that adds content after the
 * first measure (a late list), and a Save action that is refused: the frame shows the refusal in its alert slot (role=alert, icon,
 * danger tint, Try again). The frame must follow both, whatever the kind says it does.
 */
@Component({
  selector: 'sd-fixture-dialog',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <article class="text">
      <h2>Size fixture</h2>
      <p>Content that arrives after the window was measured.</p>
      <p class="tryit">
        <button type="button" data-testid="fixture-add" (click)="addRows()">Add content</button>
      </p>
      <ul data-testid="fixture-list">
        @for (n of rows(); track n) {
          <li>Line {{ n }}: a sentence long enough to wrap at the narrow width, so more of them make the content taller.</li>
        }
      </ul>
    </article>
  `,
  styleUrl: './dialog-content.css',
})
export class FixtureDialog implements DialogContent {
  protected readonly rows = signal<readonly number[]>([]);

  /** Save is always refused, with a long message: the frame draws it as its alert. */
  readonly dialogActions = computed<readonly DialogAction[]>(() => [
    {
      kind: 'run',
      id: 'save',
      label: 'Save',
      role: 'primary',
      run: () => Promise.reject(new DialogActionError("Couldn't save: the engine did not answer. The request timed out after 30 seconds and nothing was changed. Try again, or check that the agent is running.")),
    },
  ]);

  protected addRows(): void {
    this.rows.update((r) => [...r, ...Array.from({ length: 6 }, (_, i) => r.length + i + 1)]);
  }
}
