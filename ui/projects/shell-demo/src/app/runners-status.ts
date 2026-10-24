import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { RunnerData } from './runner-data';

/** What the item says: the exceptions in the table's words, or that every runner is online. */
export function runnersStatusText(offline: number, updating: number, total: number): string {
  const parts = [offline ? `${offline} offline` : '', updating ? `${updating} updating` : ''].filter(Boolean);
  return parts.length ? parts.join(' · ') : `All ${total} runners online`;
}

/** The demo's status item (start side, lowest priority: the first to give way when the bar is narrow). */
@Component({
  selector: 'sd-runners-status',
  changeDetection: ChangeDetectionStrategy.OnPush,
  // The table's own words, exceptions only ("1 offline · 1 updating"); the whole count in the tooltip and name.
  template: `<span class="text" data-testid="runners-status" [attr.title]="detail" [attr.aria-label]="detail">{{ text }}</span>`,
  styles: `:host { display: inline-flex; min-width: 0; } .text { white-space: nowrap; font-variant-numeric: tabular-nums; }`,
})
export class RunnersStatus {
  private readonly data = inject(RunnerData);
  protected readonly text = runnersStatusText(this.data.offline, this.data.updating, this.data.runners.length);
  protected readonly detail = `${this.data.online} of ${this.data.runners.length} runners online, ${this.data.updating} updating, ${this.data.offline} offline`;
}
