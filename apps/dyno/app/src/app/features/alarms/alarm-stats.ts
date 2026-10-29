import { ruleLabel } from './alarms-model';
import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { PdCell, PdTable, type PdColumn, type PdRowAction } from '@decryp7/paddock-ui/table';
import { AsyncState } from '../../shared/async-state';
import { formatDuration } from '../../shared/format';
import { AlarmsStore } from '../../state/alarms.store';

interface NoisyRule {
  readonly rule: string;
  readonly occurrences: number;
}

/** How the built-in alerts behave: time to acknowledge, the noisiest rules (with Mute) and the engine's suggestions. */
@Component({
  selector: 'dy-alarm-stats',
  imports: [AsyncState, PdTable, PdCell],
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
                <pd-table [toolbar]="false" label="Noisiest rules" [rows]="s.noisiest" [columns]="columns" [rowKey]="ruleKey" [filterable]="false" [rowActions]="actions" [maxHeight]="360" (action)="mute($event.row)">
                  <ng-template pdCell="rule" let-n><span class="id">{{ ruleName(n.rule) }}</span>@if (muted(n.rule)) {<span class="faint sfx">Muted</span>}</ng-template>
                </pd-table>
              }
            </div>
            <div>
              <h3>Suggestions</h3>
              @if (s.suggestions.length === 0) {
                <p class="lead">Nothing to adjust. Your alerts look healthy.</p>
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
  styles: `
    :host { display: block; }
    .sec-head { display: grid; gap: 2px; margin-bottom: var(--sp-3); }
    .heroes { display: flex; gap: var(--sp-8); margin-bottom: var(--sp-4); }
    .hero { display: grid; gap: 2px; }
    .big { font: var(--fw-semibold) var(--fs-2xl) / 1.15 var(--font-sans); }
    .cap { color: var(--text-3); font-size: var(--fs-sm); }
    .cols { display: grid; grid-template-columns: minmax(0, 1.2fr) minmax(0, 1fr); gap: var(--sp-6); align-items: start; }
    h3 { margin: 0 0 var(--sp-2); font: var(--fw-semibold) var(--fs-base) / 1.3 var(--font-sans); }
    .sfx { margin-inline-start: var(--sp-2); font-size: var(--fs-xs); color: var(--text-3); }
    .id { font-size: var(--fs-sm); line-height: 1.2; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .sugg { margin: 0; padding-left: var(--sp-5); display: grid; gap: var(--sp-2); font-size: var(--fs-base); color: var(--text-2); }
    @media (max-width: 1000px) { .cols { grid-template-columns: minmax(0, 1fr); } }
  `,
})
export class AlarmStats {
  protected readonly store = inject(AlarmsStore);
  protected readonly ruleName = ruleLabel;
  protected readonly dur = (ms: number | null) => formatDuration(ms === null ? null : ms / 1000);
  protected readonly ruleKey = (n: NoisyRule): string => n.rule;
  protected readonly columns: PdColumn<NoisyRule>[] = [
    { key: 'rule', header: 'Rule', value: (n) => ruleLabel(n.rule), required: true, sortable: false, width: 'minmax(0, 1fr)' },
    { key: 'fired', header: 'Fired', value: (n) => n.occurrences, text: (n) => `×${String(n.occurrences)}`, align: 'end', sortable: false, width: '70px' },
  ];
  protected readonly actions = (n: NoisyRule): PdRowAction[] => [{ id: 'mute', label: this.muted(n.rule) ? 'Unmute this rule' : 'Mute this rule', icon: 'bell', inline: true }];
  protected mute(n: NoisyRule): void {
    void this.store.mute(n.rule, !this.muted(n.rule));
  }
  protected readonly muted = (rule: string) => this.store.mutedRules().has(rule);
}
