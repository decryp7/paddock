import { Pipe, PipeTransform, inject } from '@angular/core';
import { ClockService } from '../../core/clock.service';
import { agoShort, relativeTime, spanText } from '../format';

/** `{{ ms | ago }}`: "3 min ago" / "in 2 h", refreshed by the clock signal (hence impure). */
@Pipe({ name: 'ago', pure: false })
export class AgoPipe implements PipeTransform {
  private readonly clock = inject(ClockService);

  transform(ms: number | null | undefined): string {
    return relativeTime(ms, this.clock.now());
  }
}

/** `{{ ms | agoShort }}`: "20 min", "3 h" for table cells that must not wrap. */
@Pipe({ name: 'agoShort', pure: false })
export class AgoShortPipe implements PipeTransform {
  private readonly clock = inject(ClockService);

  transform(ms: number | null | undefined): string {
    return agoShort(ms, this.clock.now());
  }
}

/** `{{ ms | untilShort }}`: "6 h" for a time ahead, "now" when it is due. */
@Pipe({ name: 'untilShort', pure: false })
export class UntilShortPipe implements PipeTransform {
  private readonly clock = inject(ClockService);

  transform(ms: number | null | undefined): string {
    if (ms === null || ms === undefined) return '—';
    const diff = ms - this.clock.now();
    return diff < 45_000 ? 'now' : spanText(diff);
  }
}
