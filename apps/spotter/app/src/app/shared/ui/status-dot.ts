import { ChangeDetectionStrategy, Component, computed, input } from '@angular/core';
import type { WatchSummary } from '../../core/api.types';
import { statusView } from '../labels';

/** Calm status: a dot while everything is fine, a dot and a sentence-case word when something needs a look. */
@Component({
  selector: 'sp-status-dot',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `<span class="dot" [class]="view().tone" [attr.title]="view().word" role="img" [attr.aria-label]="view().word"></span
    >@if (showWord()) {<span class="word" [class]="view().tone">{{ view().word }}</span>}`,
  styles: `
    :host {
      display: inline-flex;
      align-items: center;
      gap: var(--pd-space-2);
      white-space: nowrap;
    }
    .dot {
      width: 8px;
      height: 8px;
      border-radius: 50%;
      background: var(--pd-text-subtle);
    }
    .dot.success { background: var(--pd-success); }
    .dot.warning { background: var(--pd-warning); }
    .dot.danger { background: var(--pd-danger); }
    .dot.info { background: var(--pd-info); }
    .word { color: var(--pd-text-muted); }
    .word.danger { color: var(--pd-danger); }
  `,
})
export class StatusDot {
  readonly watch = input.required<Pick<WatchSummary, 'status' | 'paused' | 'checking'>>();
  /** Show the word even when healthy (the detail header does). */
  readonly always = input(false);
  protected readonly view = computed(() => statusView(this.watch()));
  protected readonly showWord = computed(() => this.always() || this.view().tone !== 'success');
}
