import { ChangeDetectionStrategy, Component, OnDestroy, OnInit, computed, inject, signal } from '@angular/core';
import { MatButton } from '@angular/material/button';
import { PdCell, PdTable, type PdColumn, type PdSort } from '@decryp7/paddock-ui/table';
import type { ThrottleEpisode } from '../../core/api.types';
import { FullTextTip } from '../../shared/full-text-tip';
import { AsyncState } from '../../shared/async-state';
import { formatDuration, formatStamp } from '../../shared/format';
import { Icon } from '../../shared/icon';
import { ThrottleStore } from '../../state/throttle.store';
import { CAUSE_LABEL, CAUSE_TONE, confidenceText, episodeSeconds, verdictTone } from './throttling-model';

/**
 * Throttling: whether the CPU is running slower than it should and why. One reading now (words, confidence and the numbers
 * behind it), one table of the last 30 days, and what this machine reports, so "cannot tell" explains itself. The engine
 * decides everything; this page only lays it out. It asks again every 15 seconds while it is open.
 */
@Component({
  selector: 'dy-throttling-page',
  imports: [FullTextTip, MatButton, AsyncState, Icon, PdTable, PdCell],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="page">
      <header class="page-head">
        <div class="titles">
          <h1>Throttling</h1>
          <p>Whether the CPU is running slower than it should, and why. Dyno only reads; it changes nothing.</p>
        </div>
      </header>

      <dy-async [loaded]="!!overview()" [error]="store.error()" what="the throttling check" [rows]="4" (retry)="store.load()">
        @if (overview(); as d) {
          <div class="stack">
            <section class="card now" aria-label="Right now">
              <div class="status" role="status">
                <span class="dot" [attr.data-tone]="tone()" aria-hidden="true"></span>
                <h2>{{ d.reading.headline }}</h2>
              </div>
              <p class="why">{{ d.reading.explanation }}</p>
              @if (d.current; as c) {
                <p class="current num">Slowed for {{ duration(c) }}, since {{ stamp(c.start) }}</p>
              }
              <p class="conf">{{ confidence() }}</p>
              @if (d.reading.evidence.length) {
                <dl class="kv" aria-label="What this is based on">
                  @for (e of d.reading.evidence; track e.label) {
                    <div><dt>{{ e.label }}</dt><dd class="num">{{ e.value }}</dd></div>
                  }
                </dl>
              }
            </section>

            <section class="card" aria-label="Last 30 days">
              <div class="card-head">
                <h2><dy-icon name="calendar" /> Last 30 days</h2>
                @if (d.episodes.length) {
                  <span class="summary num">{{ summary() }}</span>
                }
              </div>
              @if (d.episodes.length) {
                <div class="scroll">
                  <pd-table [toolbar]="false" label="Times the CPU was slowed down in the last 30 days" [rows]="d.episodes" [columns]="columns" [rowKey]="rowKey" [filterable]="false" [maxHeight]="420" [(sort)]="sort">
                    <ng-template pdCell="cause" let-e><span class="dot" [attr.data-tone]="causeToneOf(e)" aria-hidden="true"></span><span class="ell" [dyFullText]="causeLabelOf(e)">{{ causeLabelOf(e) }}</span></ng-template>
                  </pd-table>
                </div>
              } @else {
                <div class="empty">
                  <dy-icon name="check" />
                  <strong>No throttling in the last 30 days</strong>
                  <span>The CPU kept up its speed whenever it was busy. If that changes, it will be listed here.</span>
                </div>
              }
            </section>

            <section class="inputs" aria-label="What Dyno can see on this machine">
              <button type="button" matButton="text" [attr.aria-expanded]="inputsOpen()" aria-controls="throttle-inputs" (click)="userOpen.set(!inputsOpen())">
                <dy-icon [name]="inputsOpen() ? 'chevron-down' : 'chevron-right'" /> What Dyno can see on this machine
              </button>
              @if (inputsOpen()) {
                <ul id="throttle-inputs">
                  @for (i of d.inputs; track i.label) {
                    <li>
                      <span class="dot" [attr.data-tone]="i.available ? 'ok' : 'muted'" aria-hidden="true"></span>
                      <strong>{{ i.label }}</strong>
                      <span class="avail">{{ i.available ? 'Available' : 'Not available' }}</span>
                      <span class="detail">{{ i.detail }}</span>
                    </li>
                  }
                </ul>
              }
            </section>
          </div>
        }
      </dy-async>
    </div>
  `,
  styles: `
    :host { display: block; }
    .page { display: grid; gap: var(--sp-4); align-content: start; }
    .page-head { margin-bottom: 0; }
    .stack { display: grid; gap: var(--sp-4); min-width: 0; align-content: start; }
    .num { font-variant-numeric: tabular-nums; }
    .now { padding: var(--sp-4) var(--sp-5); display: grid; gap: var(--sp-2); }
    .status { display: flex; align-items: center; gap: var(--sp-3); }
    .status h2 { font-size: 15px; font-weight: var(--fw-semibold); margin: 0; }
    .why { margin: 0; color: var(--text-2); max-width: 78ch; }
    .current { margin: 0; font-weight: var(--fw-medium); }
    .conf { margin: 0; color: var(--text-3); font-size: var(--fs-sm); }
    .kv { margin: var(--sp-2) 0 0; padding-top: var(--sp-3); border-top: 1px solid var(--border-subtle); display: grid; grid-template-columns: repeat(auto-fit, minmax(190px, 1fr)); gap: var(--sp-3) var(--sp-5); }
    .kv dt { color: var(--text-3); font-size: var(--fs-sm); }
    .kv dd { margin: 2px 0 0; font-weight: var(--fw-medium); }
    .card-head .summary { margin-left: auto; color: var(--text-3); font-size: var(--fs-sm); }
    .scroll { margin: 0 var(--sp-4) var(--sp-3); }
    .ell { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .dot { display: inline-block; width: 8px; height: 8px; border-radius: 50%; margin-right: var(--sp-2); background: var(--border-strong); flex: none; }
    .dot[data-tone='ok'] { background: var(--ok); }
    .dot[data-tone='warn'] { background: var(--warn); }
    .dot[data-tone='bad'] { background: var(--bad); }
    .dot[data-tone='info'] { background: var(--info); }
    .empty { display: grid; justify-items: center; gap: var(--sp-1); padding: var(--sp-8) var(--sp-4); text-align: center; color: var(--text-2); }
    .empty dy-icon { --icon-size: 32px; color: var(--text-3); margin-bottom: var(--sp-1); }
    .empty strong { color: var(--text-1); font-size: 15px; font-weight: var(--fw-semibold); }
    .empty span { font-size: var(--fs-sm); max-width: 48ch; }
    .inputs ul { list-style: none; margin: var(--sp-2) 0 0; padding: 0 var(--sp-3); display: grid; }
    .inputs li { display: flex; align-items: baseline; gap: var(--sp-3); padding: var(--sp-2) 0; border-top: 1px solid var(--border-subtle); font-size: var(--fs-sm); }
    .inputs li:first-child { border-top: 0; }
    .inputs .dot { align-self: center; }
    .inputs strong { font-weight: var(--fw-medium); min-width: 200px; }
    .inputs .avail { color: var(--text-3); min-width: 96px; }
    .inputs .detail { color: var(--text-2); min-width: 0; }
  `,
})
export class ThrottlingPage implements OnInit, OnDestroy {
  protected readonly store = inject(ThrottleStore);
  protected readonly overview = this.store.overview;
  protected readonly causeLabel = CAUSE_LABEL;
  protected readonly causeTone = CAUSE_TONE;
  /** The episodes arrive newest first; the table sorts them (Started, newest first, until the person chooses another order). */
  protected readonly sort = signal<PdSort | null>({ key: 'start', direction: 'desc' });
  protected readonly rowKey = (e: ThrottleEpisode): string => e.id;
  protected readonly columns: PdColumn<ThrottleEpisode>[] = [
    { key: 'start', header: 'Started', value: (e) => e.start, text: (e) => this.stamp(e.start), width: '150px' },
    { key: 'duration', header: 'Duration', value: (e) => episodeSeconds(e, this.overview()?.at ?? e.start), text: (e) => this.duration(e), align: 'end', width: '80px' },
    { key: 'cause', header: 'Cause', value: (e) => CAUSE_LABEL[e.cause], width: 'minmax(120px, 1fr)' },
    { key: 'lost', header: 'Speed lost, average (%)', value: (e) => e.avgLostPercent, align: 'end', width: '168px' },
    { key: 'peak', header: 'Peak temperature (°C)', value: (e) => e.peakTemperatureC, text: (e) => (e.peakTemperatureC === null ? '—' : String(e.peakTemperatureC)), align: 'end', width: '150px' },
  ];
  protected readonly userOpen = signal<boolean | null>(null);

  protected readonly tone = computed(() => {
    const d = this.overview();
    return d ? verdictTone(d.reading.verdict) : 'muted';
  });
  protected readonly confidence = computed(() => confidenceText(this.overview()?.reading.confidence ?? 'low'));
  protected readonly summary = computed(() => `${formatDuration((this.overview()?.minutes30d ?? 0) * 60)} throttled in the last 30 days`);
  /** The list of what the machine reports is open on its own when Dyno cannot tell, and stays as the user left it otherwise. */
  protected readonly inputsOpen = computed(() => this.userOpen() ?? this.overview()?.reading.verdict === 'cannotTell');

  ngOnInit(): void {
    void this.store.start();
  }

  ngOnDestroy(): void {
    this.store.stop();
  }

  protected causeToneOf = (e: ThrottleEpisode) => CAUSE_TONE[e.cause];
  protected causeLabelOf = (e: ThrottleEpisode) => CAUSE_LABEL[e.cause];
  protected stamp = (at: number): string => formatStamp(at, this.overview()?.at ?? at);
  protected duration = (e: ThrottleEpisode): string => formatDuration(episodeSeconds(e, this.overview()?.at ?? e.start));
}
