import { ChangeDetectionStrategy, Component, computed, input } from '@angular/core';
import { SuStatusPill } from '@decryp7/paddock-ui';
import type { WatchSummary } from '../../core/api.types';
import { statusView } from '../labels';

/** A watch's status as icon shape, colour and word (never colour alone). */
@Component({
  selector: 'sp-watch-status',
  imports: [SuStatusPill],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `<pd-status-pill [status]="view().tone" [label]="view().word" />`,
})
export class WatchStatus {
  readonly watch = input.required<Pick<WatchSummary, 'status' | 'paused' | 'checking'>>();
  protected readonly view = computed(() => statusView(this.watch()));
}
