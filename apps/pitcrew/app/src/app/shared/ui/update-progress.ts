import { ChangeDetectionStrategy, Component, computed, input } from '@angular/core';
import { MatIcon } from '@angular/material/icon';
import type { UpdatePhase } from '../../core/api.types';
import { formatDuration } from '../format';
import { PHASE_STEPS, phaseIndex, phaseLabel, rollbackReasonLabel } from '../labels';

/** The update flow as steps: download → verify → wait for idle → stop → swap → confirm (or rolling back). */
@Component({
  selector: 'pc-update-progress',
  imports: [MatIcon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @let p = phase();
    <div class="head">
      <span class="what" [class.tone-warning]="p.phase === 'rollingBack'">
        <mat-icon [svgIcon]="p.phase === 'rollingBack' ? 'pc:undo' : 'pc:loader'" aria-hidden="true" [class.spin]="p.phase !== 'rollingBack'" />
        {{ label() }}
      </span>
      @if (plan(); as pl) {
        <span class="faint num">{{ pl }}</span>
      }
    </div>
    <ol class="steps" [attr.aria-label]="'Update progress: step ' + (index() + 1) + ' of ' + steps.length">
      @for (s of steps; track s.phase; let i = $index) {
        <li [class.done]="i < index()" [class.current]="i === index()" [attr.aria-current]="i === index() ? 'step' : null">
          <span class="bar"></span>
          <span class="name">{{ s.label }}</span>
        </li>
      }
    </ol>
  `,
  styles: `
    :host {
      display: grid;
      gap: var(--space-2);
    }
    .head {
      display: flex;
      justify-content: space-between;
      gap: var(--space-3);
      font-size: var(--text-sm);
    }
    .what {
      display: inline-flex;
      align-items: center;
      gap: 6px;
      font-weight: var(--weight-medium);
      color: var(--info);
    }
    .what mat-icon {
      width: 14px;
      height: 14px;
    }
    .steps {
      display: grid;
      grid-template-columns: repeat(6, minmax(0, 1fr));
      gap: 4px;
      margin: 0;
      padding: 0;
      list-style: none;
    }
    li {
      display: grid;
      gap: 4px;
      font-size: var(--text-xs);
      color: var(--text-3);
    }
    .bar {
      height: 4px;
      border-radius: 2px;
      background: var(--surface-3);
    }
    .done .bar {
      background: var(--chart-1);
    }
    .current .bar {
      background: linear-gradient(90deg, var(--chart-1), color-mix(in srgb, var(--chart-1) 35%, var(--surface-3)));
      animation: pc-pulse 1.6s ease-in-out infinite;
    }
    .current .name {
      color: var(--text);
      font-weight: var(--weight-medium);
    }
    .done .name {
      color: var(--text-2);
    }
  `,
})
export class UpdateProgress {
  readonly phase = input.required<UpdatePhase>();
  readonly now = input.required<number>();

  protected readonly steps = PHASE_STEPS;
  protected readonly index = computed(() => phaseIndex(this.phase()));
  protected readonly plan = computed(() => {
    const p = this.phase();
    return p.phase === 'idle' ? null : `${p.plan.from ?? '—'} → ${p.plan.to}`;
  });
  protected readonly label = computed(() => {
    const p = this.phase();
    if (p.phase === 'rollingBack') return `Rolling back: ${rollbackReasonLabel(p.reason)}`;
    const since = 'since' in p ? ` · ${formatDuration(this.now() - p.since)}` : '';
    return `${phaseLabel(p)}${since}`;
  });
}
