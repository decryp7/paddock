import { ChangeDetectionStrategy, Component, OnInit, computed, inject } from '@angular/core';
import { AsyncState } from '../../shared/async-state';
import { plural } from '../../shared/format';
import { Icon } from '../../shared/icon';
import { ExtrasStore } from '../../state/extras.store';
import { PREVENTS_LABEL, heldForText, wakeTimeText } from './extras-model';
import { EXTRAS_STYLES } from './extras-styles';
import { MatTooltip } from '@angular/material/tooltip';

/** Why the computer will not sleep (what holds it awake and for how long) and what woke it recently. */
@Component({
  selector: 'dy-extras-sleep',
  imports: [MatTooltip, AsyncState, Icon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <dy-async [loaded]="!!data()" [error]="store.sleep().error" what="sleep information" [rows]="5" (retry)="store.loadSleep()">
      @if (data(); as s) {
        <h2 class="sec">Keeping this computer awake @if (!s.blockersUnavailable) { <span class="faint muted">{{ plural(s.blockers.length, 'program') }}</span> }</h2>
        @if (s.blockersUnavailable; as why) {
          <div class="none"><dy-icon name="info" /><strong>Could not read what keeps this computer awake</strong><span>{{ why }}</span></div>
        } @else if (s.blockers.length === 0) {
          <div class="none"><dy-icon name="check" /><strong>Nothing is keeping this computer awake</strong><span>It can go to sleep when it is idle.</span></div>
        } @else {
          <div class="tablewrap">
            <table class="table tbl" aria-label="Keeping this computer awake">
              <thead><tr><th>Program</th><th class="w-prev">It prevents</th><th>Reason</th><th class="w-held r">Held for</th></tr></thead>
              <tbody>
                @for (b of s.blockers; track b.name + $index) {
                  <tr>
                    <td><span class="ell" [matTooltip]="b.name"><strong class="name">{{ b.name }}</strong></span></td>
                    <td>{{ prevents(b.prevents) }}</td>
                    <td class="muted"><span class="ell" [matTooltip]="b.reason">{{ b.reason }}</span></td>
                    <td class="r num muted">{{ held(b.heldFor) }}</td>
                  </tr>
                }
              </tbody>
            </table>
          </div>
        }

        <h2 class="sec">Recent wakes @if (!s.wakesUnavailable) { <span class="faint muted">{{ plural(s.wakes.length, 'wake') }}</span> }</h2>
        @if (s.wakesUnavailable; as why) {
          <div class="none"><dy-icon name="info" /><strong>Could not read the recent wakes</strong><span>{{ why }}</span></div>
        } @else if (s.wakes.length === 0) {
          <div class="none"><dy-icon name="clock" /><strong>No wakes recorded</strong><span>The system log holds no wake events for the recent past.</span></div>
        } @else {
          <div class="tablewrap">
            <table class="table tbl" aria-label="Recent wakes">
              <thead><tr><th class="w-when">When</th><th>What woke it</th></tr></thead>
              <tbody>
                @for (w of s.wakes; track $index) {
                  <tr><td class="num muted">{{ when(w.when) }}</td><td><span class="ell" [matTooltip]="w.reason">{{ w.reason }}</span></td></tr>
                }
              </tbody>
            </table>
          </div>
        }
      }
    </dy-async>
  `,
  styles: [EXTRAS_STYLES, `.name { font-weight: var(--fw-medium); } .w-prev { width: 220px; } .w-held { width: 110px; } .w-when { width: 180px; }`],
})
export class ExtrasSleep implements OnInit {
  protected readonly store = inject(ExtrasStore);
  protected readonly data = computed(() => this.store.sleep().data);
  protected readonly plural = plural;
  protected readonly prevents = (p: keyof typeof PREVENTS_LABEL) => PREVENTS_LABEL[p];
  protected readonly held = heldForText;
  protected readonly when = wakeTimeText;

  ngOnInit(): void {
    void this.store.ensure('sleep');
  }
}
