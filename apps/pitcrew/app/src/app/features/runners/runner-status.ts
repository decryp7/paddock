import { ChangeDetectionStrategy, Component, computed, input } from '@angular/core';
import type { RunnerStatus } from '../../core/api.types';
import { statusLook } from '../../shared/labels';

/** A runner's status as an 8 px dot and a sentence-case word (no pill). Idle is muted; colour appears on exception. */
@Component({
  selector: 'pc-runner-status',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `<span class="dot" [class.pulse]="status() === 'busy'" [class]="'dot tone-' + tone()" aria-hidden="true"></span><span class="word" [class.muted-word]="status() === 'idle'" [attr.title]="look().hint">{{ look().label }}</span>`,
  styles: `
    :host {
      display: inline-flex;
      align-items: center;
      gap: 6px;
      white-space: nowrap;
    }
    .muted-word {
      color: var(--text-3);
    }
  `,
})
export class RunnerStatusText {
  readonly status = input.required<RunnerStatus>();
  protected readonly look = computed(() => statusLook(this.status()));
  /** Healthy states stay neutral; only exceptions and activity carry a colour. */
  protected readonly tone = computed(() => (this.status() === 'idle' ? 'neutral' : this.look().tone));
}
