import { ChangeDetectionStrategy, Component, inject, input } from '@angular/core';
import { MatIconModule } from '@angular/material/icon';
import type { EngineIssue } from '../../core/api.types';
import { LiveStore } from '../../core/live.store';
import { countdown } from '../../shared/format';

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
  imports: [MatIconModule],
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
            <button type="button" class="btn" [disabled]="live.isBusy()('model:' + i.modelId)" (click)="live.loadModel(i.modelId)">Retry</button>
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
      gap: var(--sp-3);
      padding: var(--sp-3) var(--sp-4);
      background: var(--warn-soft);
      border: 1px solid color-mix(in srgb, var(--warn) 32%, transparent);
      border-radius: var(--r-lg);
      animation: rise var(--dur-base) var(--ease);
      mat-icon {
        color: var(--warn);
      }
      &[data-state='failed'] {
        background: var(--bad-soft);
        border-color: color-mix(in srgb, var(--bad) 35%, transparent);
        mat-icon {
          color: var(--bad);
        }
      }
    }
    .title {
      font-size: var(--fs-sm);
      b {
        font-weight: var(--fw-semibold);
      }
    }
    .count {
      margin-left: var(--sp-2);
      color: var(--text-3);
      font-size: var(--fs-xs);
    }
    .msg {
      margin-top: 2px;
      font-size: var(--fs-xs);
      color: var(--text-2);
    }
    .when {
      font-size: var(--fs-sm);
      color: var(--text-2);
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
