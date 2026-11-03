import { ChangeDetectionStrategy, Component, signal } from '@angular/core';

/**
 * The fixture of the size-to-content checks (ADR 0043 check 4), the same for every kind: a button that adds content after the
 * first measure (a late list), and one that adds a long error. The frame must follow it, whatever the kind says it does.
 */
@Component({
  selector: 'sd-fixture-dialog',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <article class="text">
      <h2>Size fixture</h2>
      <p>Content that arrives after the window was measured.</p>
      <p class="actions">
        <button type="button" data-testid="fixture-add" (click)="addRows()">Add content</button>
        <button type="button" data-testid="fixture-error" (click)="error.set(true)">Add an error</button>
      </p>
      @if (error()) {
        <p role="alert" data-testid="fixture-alert">The engine did not answer: the request timed out after 30 seconds, and nothing was changed. Try again, or check that the agent is running.</p>
      }
      <ul data-testid="fixture-list">
        @for (n of rows(); track n) {
          <li>Line {{ n }}: a sentence long enough to wrap at the narrow width, so more of them make the content taller.</li>
        }
      </ul>
    </article>
  `,
  styleUrl: './dialog-content.css',
})
export class FixtureDialog {
  protected readonly rows = signal<readonly number[]>([]);
  protected readonly error = signal(false);

  protected addRows(): void {
    this.rows.update((r) => [...r, ...Array.from({ length: 6 }, (_, i) => r.length + i + 1)]);
  }
}
