import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { toSignal } from '@angular/core/rxjs-interop';
import { ActivatedRoute } from '@angular/router';
import { map } from 'rxjs';
import { LogsPage } from '../features/logs/logs-page';
import { RunnerDetailPage } from '../features/runners/runner-detail-page';

/** `/runners/:name?tab=`: the runner page with its route parameters as inputs (the frame's router has no input binding). */
@Component({
  selector: 'pc-runner-route',
  imports: [RunnerDetailPage],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `<pc-runner-detail-page [name]="name()" [tab]="tab()" />`,
})
export class RunnerRoute {
  private readonly route = inject(ActivatedRoute);
  protected readonly name = toSignal(this.route.paramMap.pipe(map((p) => p.get('name') ?? '')), { initialValue: this.route.snapshot.paramMap.get('name') ?? '' });
  protected readonly tab = toSignal(this.route.queryParamMap.pipe(map((q) => q.get('tab') ?? undefined)), { initialValue: this.route.snapshot.queryParamMap.get('tab') ?? undefined });
}

/** `/logs/:runner`: the log page for one runner. */
@Component({
  selector: 'pc-logs-route',
  imports: [LogsPage],
  changeDetection: ChangeDetectionStrategy.OnPush,
  styles: ':host { display: block; height: 100%; min-height: 0; }',
  template: `<pc-logs-page [runner]="runner()" />`,
})
export class LogsRoute {
  private readonly route = inject(ActivatedRoute);
  protected readonly runner = toSignal(this.route.paramMap.pipe(map((p) => p.get('runner') ?? undefined)), { initialValue: this.route.snapshot.paramMap.get('runner') ?? undefined });
}
