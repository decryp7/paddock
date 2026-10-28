import { ChangeDetectionStrategy, Component, OnInit, inject } from '@angular/core';
import type { LifeForecast, LifeOutcome } from '../../core/api.types';
import { AsyncState } from '../../shared/async-state';
import { formatDate, formatSpan } from '../../shared/format';
import { MatProgressBar } from '@angular/material/progress-bar';
import { Icon } from '../../shared/icon';
import { LifespanStore } from '../../state/lifespan.store';
import { SUBJECT_ICON, SUBJECT_WORD, confidenceText, historyProgress, outcomeKind, outcomeTone, rangeBar } from './lifespan-model';

/**
 * Disk and battery life: one row per disk or battery with the engine's forecast in words, how sure it is and the data it stands
 * on. A range bar appears only when there is a range to show. The engine decides every number; this page lays them out.
 */
@Component({
  selector: 'dy-lifespan-page',
  imports: [MatProgressBar, AsyncState, Icon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="page">
      <header class="page-head">
        <div class="titles">
          <h1>Disk and battery life</h1>
          <p>How long your SSD and battery are likely to last, from how they have worn so far. An estimate with a range, never a promise.</p>
        </div>
      </header>

      <dy-async [loaded]="!!store.overview()" [error]="store.error()" [empty]="store.overview()?.forecasts?.length === 0" emptyTitle="No disk or battery to forecast" emptyText="Dyno found no SSD or battery that reports its wear on this machine." emptyIcon="disk" what="the forecasts" [rows]="4" (retry)="store.load()">
        <section class="card" aria-label="Forecasts">
          <ul class="list">
            @for (f of store.overview()?.forecasts ?? []; track f.name) {
              <li [attr.data-subject]="f.subject">
                <div class="top">
                  <span class="ic"><dy-icon [name]="icon[f.subject]" /></span>
                  <h2>{{ f.name }}</h2>
                  <span class="conf">{{ confidence(f) }}</span>
                </div>

                <p class="headline"><span class="dot" [attr.data-tone]="tone(f.outcome)" aria-hidden="true"></span>{{ f.headline }}</p>

                @switch (kind(f.outcome)) {
                  @case ('remaining') {
                    @if (bar(f.outcome); as b) {
                      <div class="range" role="img" [attr.aria-label]="rangeLabel(f.outcome)">
                        <div class="track">
                          <span class="span" [style.left.%]="b.lowPct" [style.width.%]="100 - b.lowPct"></span>
                          <span class="mark" [style.left.%]="b.midPct"></span>
                        </div>
                        <div class="ends num">
                          <span>Soonest {{ span(low(f.outcome)) }}</span>
                          <span class="mid">Expected {{ span(mid(f.outcome)) }}</span>
                          <span>Latest {{ span(high(f.outcome)) }}</span>
                        </div>
                      </div>
                    }
                  }
                  @case ('tooLittleHistory') {
                    @if (history(f.outcome); as h) {
                      <div class="progress">
                        <mat-progress-bar class="bar" mode="determinate" aria-label="History collected" [value]="progress(f.outcome)" />
                        <span class="num">Have {{ h.haveDays }} of {{ h.needDays }} days</span>
                      </div>
                    }
                  }
                  @case ('unavailable') {
                    <p class="unavailable"><dy-icon name="info" />{{ reason(f.outcome) }}</p>
                  }
                }

                @if (f.explanation && kind(f.outcome) !== 'unavailable') {
                  <p class="why">{{ f.explanation }}</p>
                }
                @if (f.restartedAt !== null) {
                  <p class="note">A different {{ word[f.subject] }} was detected on {{ date(f.restartedAt) }}; only readings since then are used.</p>
                }
                @if (f.basis.length) {
                  <dl class="kv" [attr.aria-label]="'What this stands on for ' + f.name">
                    @for (e of f.basis; track e.label) {
                      <div><dt>{{ e.label }}</dt><dd class="num">{{ e.value }}</dd></div>
                    }
                  </dl>
                }
              </li>
            }
          </ul>
        </section>
      </dy-async>
    </div>
  `,
  styles: `
    :host { display: block; }
    .page { display: grid; gap: var(--sp-4); align-content: start; }
    .page-head { margin-bottom: 0; }
    .num { font-variant-numeric: tabular-nums; }
    .list { list-style: none; margin: 0; padding: 0 var(--sp-5); display: grid; }
    .list li { display: grid; gap: var(--sp-2); padding: var(--sp-4) 0; border-top: 1px solid var(--border-subtle); }
    .list li:first-child { border-top: 0; }
    .top { display: flex; align-items: center; gap: var(--sp-3); }
    .top h2 { margin: 0; font-size: var(--fs-base); font-weight: var(--fw-semibold); flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .ic { display: grid; place-items: center; width: 28px; height: 28px; border-radius: var(--r-md); background: var(--surface-active); color: var(--text-2); }
    .conf { color: var(--text-3); font-size: var(--fs-sm); white-space: nowrap; }
    .headline { margin: 0; display: flex; align-items: center; gap: var(--sp-2); font-size: 15px; font-weight: var(--fw-semibold); }
    .range { display: grid; gap: var(--sp-1); max-width: 560px; }
    .track { position: relative; height: 8px; border-radius: var(--r-pill); background: var(--surface-active); }
    .track .span { position: absolute; top: 0; bottom: 0; border-radius: var(--r-pill); background: var(--border-strong); }
    .track .mark { position: absolute; top: -3px; width: 4px; height: 14px; margin-left: -2px; border-radius: 2px; background: var(--accent); }
    .ends { display: flex; justify-content: space-between; gap: var(--sp-3); color: var(--text-3); font-size: var(--fs-sm); white-space: nowrap; }
    .ends .mid { color: var(--text-1); font-weight: var(--fw-medium); }
    .progress { display: flex; align-items: center; gap: var(--sp-3); color: var(--text-2); font-size: var(--fs-sm); max-width: 560px; }
    .progress .bar { flex: 1; }
    .progress .num { white-space: nowrap; }
    .unavailable { margin: 0; display: flex; gap: var(--sp-2); align-items: flex-start; color: var(--text-2); max-width: 78ch; }
    .unavailable dy-icon { --icon-size: 16px; color: var(--text-3); margin-top: 2px; }
    .why { margin: 0; color: var(--text-2); max-width: 78ch; }
    .note { margin: 0; color: var(--text-3); font-size: var(--fs-sm); }
    .kv { margin: var(--sp-1) 0 0; display: grid; grid-template-columns: repeat(auto-fit, minmax(190px, 1fr)); gap: var(--sp-2) var(--sp-5); }
    .kv dt { color: var(--text-3); font-size: var(--fs-sm); }
    .kv dd { margin: 2px 0 0; font-weight: var(--fw-medium); }
  `,
})
export class LifespanPage implements OnInit {
  protected readonly store = inject(LifespanStore);
  protected readonly icon = SUBJECT_ICON;
  protected readonly word = SUBJECT_WORD;
  protected readonly kind = outcomeKind;
  protected readonly tone = outcomeTone;
  protected readonly span = formatSpan;
  protected readonly confidence = (f: LifeForecast) => confidenceText(f.confidence) + (f.confidence === 'low' ? ' (a rough guide)' : '');
  protected readonly date = (at: number) => formatDate(at, this.store.overview()?.at ?? at);

  private remaining(o: LifeOutcome) {
    return typeof o === 'object' && 'remaining' in o ? o.remaining : null;
  }
  protected bar(o: LifeOutcome) {
    const r = this.remaining(o);
    return r ? rangeBar(r) : null;
  }
  protected low = (o: LifeOutcome) => this.remaining(o)?.lowDays ?? 0;
  protected mid = (o: LifeOutcome) => this.remaining(o)?.midDays ?? 0;
  protected high = (o: LifeOutcome) => this.remaining(o)?.highDays ?? 0;
  protected rangeLabel = (o: LifeOutcome) => `Soonest ${formatSpan(this.low(o))}, expected ${formatSpan(this.mid(o))}, latest ${formatSpan(this.high(o))}`;
  protected history(o: LifeOutcome) {
    return typeof o === 'object' && 'tooLittleHistory' in o ? o.tooLittleHistory : null;
  }
  protected progress = (o: LifeOutcome) => {
    const h = this.history(o);
    return h ? historyProgress(h) : 0;
  };
  protected reason = (o: LifeOutcome) => (typeof o === 'object' && 'unavailable' in o ? o.unavailable.reason : '');

  ngOnInit(): void {
    void this.store.load();
  }
}
