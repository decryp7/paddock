import { ChangeDetectionStrategy, Component, inject, input } from '@angular/core';
import { MatIconModule } from '@angular/material/icon';
import type { EngineIssue } from '../../core/api.types';
import { LiveStore } from '../../core/live.store';
import { countdown } from '../../shared/format';
import { MatButton } from '@angular/material/button';

/** Text for the right-hand side of an issue: a countdown to the restart, "restarting now", or nothing for a failed model. */
export function restartText(issue: EngineIssue, now: number): string | null {
  if (issue.state === 'failed') return null;
  return issue.restartAt === null ? 'Restarting now' : `Restarts in ${countdown(issue.restartAt, now)}`;
}

/**
 * Models whose engine crashed: restarting (with a countdown) or given up on after a crash loop, with Retry. Shown at
 * the top of the dashboard only while something is wrong.
 */
@Component({
  selector: 'pw-engine-issues',
  imports: [MatButton, MatIconModule],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @for (i of issues(); track i.modelId) {
      <section class="issue" [attr.data-state]="i.state" role="alert" [attr.aria-label]="'Engine problem: ' + i.modelId">
        <mat-icon [svgIcon]="i.state === 'failed' ? 'pw:error' : 'pw:alert'" aria-hidden="true" />
        <div class="text">
          <p class="title">
            <b>{{ i.modelId }}</b>
            {{ i.state === 'failed' ? 'keeps crashing' : 'stopped unexpectedly' }}
            <span class="count num">{{ i.crashes }} {{ i.crashes === 1 ? 'crash' : 'crashes' }}</span>
          </p>
          <p class="msg">{{ i.message }}</p>
        </div>
        <div class="side">
          @if (restartText(i, now()); as t) {
            <span class="when num">{{ t }}</span>
          } @else {
            <button type="button" matButton="outlined" [disabled]="live.isBusy()('model:' + i.modelId)" (click)="live.loadModel(i.modelId)">Retry</button>
          }
        </div>
      </section>
    }
  `,
  styles: `
    :host {
      display: contents;
    }
    .issue {
      display: grid;
      grid-template-columns: auto minmax(0, 1fr) auto;
      align-items: center;
      gap: var(--pd-space-3);
      padding: var(--pd-space-3) var(--pd-space-4);
      background: var(--pd-surface-1);
      border: 1px solid var(--pd-hairline);
      border-left: 3px solid var(--pd-warning);
      border-radius: var(--pd-radius-control);
      animation: rise var(--pd-duration) var(--pd-ease-move);
      mat-icon {
        color: var(--pd-warning);
      }
      &[data-state='failed'] {
        border-left-color: var(--pd-danger);
        mat-icon {
          color: var(--pd-danger);
        }
      }
    }
    .title {
      font-size: var(--pd-text-label);
      b {
        font-weight: var(--pd-weight-semibold);
      }
    }
    .count {
      margin-left: var(--pd-space-2);
      color: var(--pd-text-subtle);
      font-size: var(--pd-text-label);
    }
    .msg {
      margin-top: 2px;
      font-size: var(--pd-text-label);
      color: var(--pd-text-muted);
    }
    .when {
      font-size: var(--pd-text-label);
      color: var(--pd-text-muted);
    }
    @keyframes rise {
      from {
        opacity: 0;
        transform: translateY(-4px);
      }
    }
    @media (prefers-reduced-motion: reduce) {
      .issue {
        animation: none;
      }
    }
  `,
})
export class EngineIssues {
  protected readonly live = inject(LiveStore);
  readonly now = input.required<number>();
  protected readonly issues = this.live.issues;
  protected readonly restartText = restartText;
}
