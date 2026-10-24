import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { GreetingLog } from './greeting-log';

/** A status item of the demo (start side, lowest priority: the first to give way when the bar is narrow). */
@Component({
  selector: 'sd-greetings-status',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `<span class="text" data-testid="greetings-status">Greetings: {{ log.greetings() }}</span>`,
  styles: `:host { display: inline-flex; min-width: 0; } .text { white-space: nowrap; font-variant-numeric: tabular-nums; }`,
})
export class GreetingsStatus {
  protected readonly log = inject(GreetingLog);
}
