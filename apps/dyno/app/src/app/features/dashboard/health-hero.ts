import { ChangeDetectionStrategy, Component, OnInit, computed, inject } from '@angular/core';
import type { HealthReason } from '../../core/api.types';
import { errorHint } from '../../core/command-error';
import { formatAgo, formatSigned } from '../../shared/format';
import { Icon } from '../../shared/icon';
import { GRADE_LABEL, GRADE_TONE, SEVERITY_LABEL, SEVERITY_TONE } from '../../shared/labels';
import { Ring } from '../../shared/ring';
import { HealthStore } from '../../state/health.store';
import { MatButton } from '@angular/material/button';
import { MatTooltip } from '@angular/material/tooltip';

/** The health score with its top reasons. */
@Component({
  selector: 'dy-health-hero',
  imports: [MatTooltip, MatButton, Ring, Icon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <section class="card" aria-label="Health score">
      @if (store.report(); as r) {
        <div class="score">
          <dy-ring [value]="r.score" [tone]="tone()" [label]="'Health score ' + score(r.score) + ' of 100, ' + grade()" style="--ring-size: 104px">
            <span class="num">{{ score(r.score) }}</span>
            <small>of 100</small>
          </dy-ring>
          <div class="who">
            <span class="eyebrow">Health</span>
            <strong class="grade" [attr.data-tone]="tone()">{{ grade() }}</strong>
            @if (trend(); as t) {
              <span class="trend num">{{ t }}</span>
            }
          </div>
        </div>
        <div class="reasons">
          <span class="eyebrow">What holds it back</span>
          @if (top().length === 0) {
            <p class="clear"><dy-icon name="check" /> Nothing needs your attention.</p>
          }
          <ul>
            @for (x of top(); track x.id) {
              <li>
                <div class="txt">
                  <span class="t"><span class="badge" [attr.data-tone]="sevTone(x)">{{ sevLabel(x) }}</span> {{ x.title }}</span>
                  @if (x.since !== null) {
                    <span class="d">Active alarm since {{ sinceText(x.since) }}. {{ x.detail }}</span>
                  } @else {
                    <span class="d">{{ x.detail }}</span>
                  }
                </div>
                @if (x.since === null) {
                  <span class="imp num" [matTooltip]="'Costs ' + pts(x.impact)">{{ pts(x.impact, true) }}</span>
                }
              </li>
            }
          </ul>
        </div>
      } @else if (store.reportError(); as e) {
        <div class="err" role="alert">
          <dy-icon name="alert" />
          <div class="text"><strong>{{ e.message }}</strong>@if (hint(); as h) {<span>{{ h }}</span>}</div>
          <button type="button" matButton="outlined" (click)="store.loadReport()"><dy-icon name="refresh" /> Retry</button>
        </div>
      } @else {
        <div class="score" aria-busy="true" aria-label="Loading health">
          <div class="skeleton ring"></div>
          <div class="who"><div class="skeleton" style="width: 80px"></div><div class="skeleton" style="width: 120px"></div></div>
        </div>
        <div class="reasons"><div class="skeleton"></div><div class="skeleton"></div><div class="skeleton" style="width: 70%"></div></div>
      }
    </section>
  `,
  styles: `
    :host { display: block; min-width: 0; }
    .card { display: grid; grid-template-columns: minmax(0, 1fr); gap: var(--sp-4); padding: var(--sp-5); align-content: start; height: 100%; align-items: center; }
    .score { display: flex; align-items: center; gap: var(--sp-4); }
    dy-ring .num { font: var(--fw-semibold) var(--fs-2xl) / 1 var(--font-display); letter-spacing: -0.03em; display: block; }
    dy-ring small { display: block; color: var(--text-3); font-size: var(--fs-xs); margin-top: 2px; }
    .who { display: grid; gap: 2px; }
    .grade { font: var(--fw-semibold) var(--fs-3xl) / 1.1 var(--font-display); }
    .grade[data-tone='ok'] { color: var(--ok); }
    .grade[data-tone='warn'] { color: var(--warn); }
    .grade[data-tone='bad'] { color: var(--bad); }
    .trend { color: var(--text-2); font-size: var(--fs-sm); }
    .reasons { display: grid; gap: var(--sp-2); min-width: 0; }
    ul { list-style: none; margin: 0; padding: 0; display: grid; gap: var(--sp-1); }
    li { display: flex; align-items: center; gap: var(--sp-3); padding: var(--sp-2) var(--sp-1); border-bottom: 1px solid var(--border-subtle); }
    .txt { flex: 1; min-width: 0; display: grid; }
    .t { font-weight: var(--fw-medium); }
    .t .badge { margin-right: 4px; vertical-align: 1px; }
    .d { color: var(--text-3); font-size: var(--fs-xs); display: -webkit-box; -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; }
    .imp { color: var(--text-3); font-size: var(--fs-sm); min-width: 28px; text-align: right; }
    .clear { display: flex; align-items: center; gap: var(--sp-2); color: var(--ok); }
    .err { grid-column: 1 / -1; display: flex; gap: var(--sp-3); align-items: flex-start; }
    .err .text { flex: 1; display: grid; }
    .err .text span { color: var(--text-2); font-size: var(--fs-sm); }
    .ring { width: 104px; height: 104px; border-radius: 50%; }
  `,
})
export class HealthHero implements OnInit {
  protected readonly store = inject(HealthStore);
  protected readonly pts = (n: number, minus = false) => (minus ? formatSigned(-n) : `${Math.round(n)} points`);
  protected readonly score = (v: number) => Math.round(v).toString();
  protected readonly grade = computed(() => {
    const r = this.store.report();
    return r ? GRADE_LABEL[r.grade] : '';
  });
  protected readonly tone = computed(() => {
    const r = this.store.report();
    return r ? GRADE_TONE[r.grade] : 'accent';
  });
  /** Every active alarm (the engine puts them first), then the three costliest performance reasons. */
  protected readonly top = computed(() => {
    const all = this.store.report()?.reasons ?? [];
    return [...all.filter((x) => x.since !== null), ...all.filter((x) => x.since === null).sort((a, b) => b.impact - a.impact).slice(0, 3)];
  });
  protected readonly trend = computed(() => {
    const w = this.store.weekly();
    const r = this.store.report();
    const change = w && w.score !== null ? `${formatSigned(w.scoreChange)} vs last week` : null;
    if (!r || r.cappedBy === null) return change;
    const held = `held at ${GRADE_LABEL[r.grade]} by an active ${SEVERITY_LABEL[r.cappedBy]} alarm (performance alone: ${Math.round(r.performanceScore)})`;
    return change ? `${change}, ${held}` : held.charAt(0).toUpperCase() + held.slice(1);
  });
  protected readonly sinceText = (at: number) => formatAgo(at, Date.now());
  protected readonly hint = computed(() => {
    const e = this.store.reportError();
    return e ? errorHint(e.code) : null;
  });
  protected readonly sevLabel = (x: HealthReason) => SEVERITY_LABEL[x.severity];
  protected readonly sevTone = (x: HealthReason) => SEVERITY_TONE[x.severity];

  ngOnInit(): void {
    void this.store.loadReport();
  }

}
