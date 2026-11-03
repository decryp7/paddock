import { FullTextTip } from '../../shared/full-text-tip';
import { ChangeDetectionStrategy, Component, computed, inject, input } from '@angular/core';
import type { HealthReason } from '../../core/api.types';
import { formatSigned } from '../../shared/format';
import { Icon } from '../../shared/icon';
import { GRADE_LABEL, GRADE_TONE, SEVERITY_LABEL, SEVERITY_TONE } from '../../shared/labels';
import { Ring } from '../../shared/ring';
import { HealthStore } from '../../state/health.store';
import { MatTooltip } from '@angular/material/tooltip';

/** The health score with its top reasons. */
@Component({
  selector: 'dy-health-hero',
  imports: [FullTextTip, MatTooltip, Ring, Icon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @if (store.report(); as r) {
      <div class="score">
        <dy-ring [value]="r.score" [tone]="tone()" [label]="'Health score ' + score(r.score) + ' of 100, ' + grade()" style="--ring-size: 104px">
          <span class="num">{{ score(r.score) }}</span>
          <small>of 100</small>
        </dy-ring>
        <div class="who">
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
                <span class="d" [dyFullText]="x.detail">{{ x.detail }}</span>
              </div>
              <span class="imp num" [matTooltip]="'Costs ' + pts(x.impact)">{{ pts(x.impact, true) }}</span>
            </li>
          }
        </ul>
      </div>
    }
  `,
  styles: `
    :host { display: grid; grid-template-columns: minmax(0, 1fr); gap: var(--sp-4); min-width: 0; align-content: start; }
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
  `,
})
export class HealthHero {
  protected readonly store = inject(HealthStore);
  /** How many of the costliest reasons to list (the widget gives fewer when it is short; the Health page lists them all). */
  readonly reasons = input(3);
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
  protected readonly top = computed(() => [...(this.store.report()?.reasons ?? [])].sort((a, b) => b.impact - a.impact).slice(0, this.reasons()));
  protected readonly trend = computed(() => {
    const w = this.store.weekly();
    return w && w.score !== null ? `${formatSigned(w.scoreChange)} vs last week` : null;
  });
  protected readonly sevLabel = (x: HealthReason) => SEVERITY_LABEL[x.severity];
  protected readonly sevTone = (x: HealthReason) => SEVERITY_TONE[x.severity];

}
