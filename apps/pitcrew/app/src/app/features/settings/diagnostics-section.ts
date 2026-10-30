import { ChangeDetectionStrategy, Component, inject, signal } from '@angular/core';
import { MatIcon } from '@angular/material/icon';
import { BACKEND } from '../../core/backend';
import { toCommandError } from '../../core/command-error';

/** Writes the redacted diagnostics bundle for a bug report. */
@Component({
  selector: 'pc-diagnostics-section',
  imports: [MatIcon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <section class="card">
      <header class="card-header"><h2>Diagnostics</h2></header>
      <div class="card-body body">
        <p class="muted">
          Writes a zip with Pitcrew’s configuration, its own logs, the service state and the last lines of each runner’s log, for a bug report.
          Tokens and registration files are never included, and secrets in log lines are masked.
        </p>
        <div class="btn-row">
          <button type="button" class="btn primary" (click)="exportDiagnostics()" [disabled]="exporting()">
            <mat-icon svgIcon="pc:package" aria-hidden="true" />{{ exporting() ? 'Writing…' : 'Export diagnostics' }}
          </button>
        </div>
        @if (exported(); as path) {
          <div class="callout success" role="status">
            <mat-icon svgIcon="pc:check-circle" aria-hidden="true" />
            <div class="callout-body">
              <strong>Saved</strong>
              <span class="mono path">{{ path }}</span>
            </div>
          </div>
        }
        @if (exportError(); as e) {
          <p class="tone-danger" role="alert">{{ e }}</p>
        }
      </div>
    </section>
  `,
  styles: `
    h2 {
      font-size: var(--text-md);
    }
    .body {
      display: grid;
      gap: var(--space-3);
    }
    .path {
      overflow-wrap: anywhere;
    }
  `,
})
export class DiagnosticsSection {
  private readonly backend = inject(BACKEND);

  protected readonly exporting = signal(false);
  protected readonly exported = signal<string | null>(null);
  protected readonly exportError = signal<string | null>(null);

  protected async exportDiagnostics(): Promise<void> {
    this.exporting.set(true);
    this.exportError.set(null);
    try {
      this.exported.set(await this.backend.exportDiagnostics());
    } catch (e) {
      this.exportError.set(toCommandError(e).message);
    } finally {
      this.exporting.set(false);
    }
  }
}
