import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { AsyncState } from '../../shared/async-state';
import { formatDuration } from '../../shared/format';
import { AlarmsStore } from '../../state/alarms.store';
import { TABLE_CSS } from './alarms-css';

/** How the built-in alerts behave: time to acknowledge, the noisiest rules (with Mute) and the engine's suggestions. */
@Component({
  selector: 'dy-alarm-stats',
  imports: [AsyncState],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <section aria-labelledby="stats-h">
      <div class="sec-head"><h2 class="h2" id="stats-h">Built-in alerts</h2><p class="lead">How often alerts fire and how quickly you answer them.</p></div>
      <dy-async [loaded]="!!store.stats()" [error]="store.statsError()" what="alarm statistics" [rows]="3" (retry)="store.loadStats()">
        @if (store.stats(); as s) {
          <div class="heroes">
            <div class="hero"><span class="big num">{{ dur(s.meanTimeToAcknowledgeMs) }}</span><span class="cap">Mean time to acknowledge</span></div>
            <div class="hero"><span class="big num">{{ dur(s.medianTimeToAcknowledgeMs) }}</span><span class="cap">Median time to acknowledge</span></div>
          </div>
          <div class="cols">
            <div>
              <h3>Noisiest rules</h3>
              @if (s.noisiest.length === 0) {
                <p class="lead">No rule has fired yet.</p>
              } @else {
                <div class="grid" role="table" aria-label="Noisiest rules">
                  <div class="head" role="row"><span role="columnheader">Rule</span><span role="columnheader" class="r">Fired</span><span role="columnheader"><span class="visually-hidden">Actions</span></span></div>
                  @for (n of s.noisiest; track n.rule) {
                    <div class="row" role="row">
                      <span class="cell" role="cell"><span class="truncate id" [title]="n.rule">{{ n.rule }}</span>@if (muted(n.rule)) {<span class="faint sfx">Muted</span>}</span>
                      <span class="cell r num" role="cell">×{{ n.occurrences }}</span>
                      <span class="cell acts" role="cell"><button type="button" class="btn sm ghost text-act" [attr.aria-label]="(muted(n.rule) ? 'Unmute ' : 'Mute ') + n.rule" (click)="store.mute(n.rule, !muted(n.rule))">{{ muted(n.rule) ? 'Unmute' : 'Mute' }}</button></span>
                    </div>
                  }
                </div>
              }
            </div>
            <div>
              <h3>Suggestions</h3>
              @if (s.suggestions.length === 0) {
                <p class="lead">Nothing to tune. Your alerts look healthy.</p>
              } @else {
                <ul class="sugg">
                  @for (g of s.suggestions; track $index) {
                    <li>{{ g.message }}</li>
                  }
                </ul>
              }
            </div>
          </div>
        }
      </dy-async>
    </section>
  `,
  styles: [
    TABLE_CSS,
    `
    :host { display: block; }
    .sec-head { display: grid; gap: 2px; margin-bottom: var(--sp-3); }
    .heroes { display: flex; gap: var(--sp-8); margin-bottom: var(--sp-4); }
    .hero { display: grid; gap: 2px; }
    .big { font: var(--fw-semibold) var(--fs-2xl) / 1.15 var(--font-sans); }
    .cap { color: var(--text-3); font-size: var(--fs-sm); }
    .cols { display: grid; grid-template-columns: minmax(0, 1.2fr) minmax(0, 1fr); gap: var(--sp-6); align-items: start; }
    h3 { margin: 0 0 var(--sp-2); font: var(--fw-semibold) var(--fs-base) / 1.3 var(--font-sans); }
    .head, .row { --cols: minmax(0, 1fr) 60px 70px; }
    .row { min-height: 36px; }
    .sfx { font-size: var(--fs-xs); }
    .text-act { opacity: 0; }
    .row:hover .text-act, .row:focus-within .text-act { opacity: 1; }
    .sugg { margin: 0; padding-left: var(--sp-5); display: grid; gap: var(--sp-2); font-size: var(--fs-base); color: var(--text-2); }
    @media (max-width: 1000px) { .cols { grid-template-columns: minmax(0, 1fr); } }
  `,
  ],
})
export class AlarmStats {
  protected readonly store = inject(AlarmsStore);
  protected readonly dur = (ms: number | null) => formatDuration(ms === null ? null : ms / 1000);
  protected readonly muted = (rule: string) => this.store.mutedRules().has(rule);
}
