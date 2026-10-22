import { Pipe, PipeTransform, inject } from '@angular/core';
import { ClockService } from '../../core/clock.service';
import { relativeTime } from '../format';

/** `{{ ms | ago }}`: "3 min ago" / "in 2 h", refreshed by the clock signal (hence impure). */
@Pipe({ name: 'ago', pure: false })
export class AgoPipe implements PipeTransform {
  private readonly clock = inject(ClockService);

  transform(ms: number | null | undefined): string {
    return relativeTime(ms, this.clock.now());
  }
}
