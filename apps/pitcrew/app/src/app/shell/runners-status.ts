import { ChangeDetectionStrategy, Component, computed, inject } from '@angular/core';
import { StatusStore } from '../state/status.store';

/** The status bar's runner summary: how many runners there are and how many need a look. */
@Component({
  selector: 'pc-runners-status',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `<span data-testid="runners-status">{{ text() }}</span>`,
  styles: `:host { font-size: var(--pd-text-xs, 11px); }`,
})
export class RunnersStatus {
  private readonly status = inject(StatusStore);
  protected readonly text = computed(() => {
    const n = this.status.runners().length;
    const a = this.status.attention().length;
    const base = n === 1 ? '1 runner' : `${n} runners`;
    return a > 0 ? `${base}, ${a} need${a === 1 ? 's' : ''} attention` : base;
  });
}
