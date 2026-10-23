import { ChangeDetectionStrategy, Component, OnInit, computed, inject } from '@angular/core';
import { AsyncState } from '../../shared/async-state';
import { formatClock, plural } from '../../shared/format';
import { Icon } from '../../shared/icon';
import { ExtrasStore } from '../../state/extras.store';
import { CHANGE_KIND_LABEL, CHANGE_WINDOWS, changeRows, changeTone, watchingNote } from './extras-model';
import { EXTRAS_STYLES } from './extras-styles';

/** What changed on this computer: installs, updates, new start-up items and setting changes, grouped by day. */
@Component({
  selector: 'dy-extras-changes',
  imports: [AsyncState, Icon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="bar" role="group" aria-label="Window">
      <div class="seg" role="group" aria-label="Period">
        @for (w of windows; track w.hours) {
          <button type="button" [attr.aria-pressed]="store.hours() === w.hours" (click)="store.setHours(w.hours)">{{ w.label }}</button>
        }
      </div>
      <span class="grow"></span>
      @if (watching()) { <span class="faint muted">{{ watching() }}</span> }
    </div>
    <dy-async [loaded]="!!data()" [error]="state().error" what="changes" [rows]="5" (retry)="store.loadChanges()">
      @if (data(); as t) {
        @if (t.changes.length === 0) {
          <div class="none">
            <dy-icon name="clock" />
            <strong>Nothing changed in this period</strong>
            <span>No installs, updates, start-up items or setting changes were recorded. {{ watching() }}</span>
          </div>
        } @else {
          <div class="tablewrap">
            <table class="table tbl changes" aria-label="What changed">
              <thead><tr><th class="time">Time</th><th class="kind">Kind</th><th>Change</th><th>Details</th></tr></thead>
              <tbody>
                @for (r of rows(); track r.key) {
                  @if (r.kind === 'day') {
                    <tr class="day"><td colspan="4"><strong>{{ r.label }}</strong><span class="faint cnt">{{ plural(r.count, 'change') }}</span></td></tr>
                  } @else {
                    <tr>
                      <td class="time num muted">{{ clock(r.change.at) }}</td>
                      <td class="kind"><span class="dot" [attr.data-tone]="tone(r.change.kind)" aria-hidden="true"></span>{{ kindLabel(r.change.kind) }}</td>
                      <td><span class="ell" [title]="r.change.title">{{ r.change.title }}</span></td>
                      <td class="muted"><span class="ell" [title]="r.change.detail">{{ r.change.detail }}</span></td>
                    </tr>
                  }
                }
              </tbody>
            </table>
          </div>
        }
      }
    </dy-async>
  `,
  styles: [
    EXTRAS_STYLES,
    `
      .changes th.time, .changes td.time { width: 72px; }
      .changes th.kind, .changes td.kind { width: 180px; }
      .changes td:nth-child(3) { width: 38%; }
    `,
  ],
})
export class ExtrasChanges implements OnInit {
  protected readonly store = inject(ExtrasStore);
  protected readonly windows = CHANGE_WINDOWS;
  protected readonly state = this.store.changes;
  protected readonly data = computed(() => this.store.changes().data);
  protected readonly rows = computed(() => {
    const t = this.data();
    return t ? changeRows(t.changes, t.to) : [];
  });
  protected readonly watching = computed(() => {
    const t = this.data();
    return t ? watchingNote(t.watchingSince, t.from, t.to) : '';
  });
  protected readonly plural = plural;
  protected readonly clock = formatClock;
  protected readonly kindLabel = (k: keyof typeof CHANGE_KIND_LABEL) => CHANGE_KIND_LABEL[k];
  protected readonly tone = changeTone;

  ngOnInit(): void {
    void this.store.ensure('changes');
  }
}
