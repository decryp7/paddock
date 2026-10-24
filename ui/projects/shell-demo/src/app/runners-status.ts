import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { RunnerData } from './runner-data';

/** The demo's status item (start side, lowest priority: the first to give way when the bar is narrow). */
@Component({
  selector: 'sd-runners-status',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `<span class="text" data-testid="runners-status">{{ data.online }} of {{ data.runners.length }} runners online</span>`,
  styles: `:host { display: inline-flex; min-width: 0; } .text { white-space: nowrap; font-variant-numeric: tabular-nums; }`,
})
export class RunnersStatus {
  protected readonly data = inject(RunnerData);
}
