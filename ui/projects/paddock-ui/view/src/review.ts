import { ChangeDetectionStrategy, Component, computed, inject, input, signal } from '@angular/core';
import { PdField, PdFieldControl } from '@decryp7/paddock-ui/forms';
import type { ActionRef, Finding, ReviewList, StateDef } from '../generated/view';
import { PdViewButton, PdViewLink } from './button';
import { toneOf } from './model';
import { ViewRuntime } from './runtime';
import { PdViewTone, TONE_CSS, toneClass } from './tone';
import { PdViewWrites } from './writes';

/** A finding with the definition of its state (null when the plugin named a state it did not define; validation refuses that). */
export interface ReviewRow {
  readonly finding: Finding;
  readonly state: StateDef | null;
}

/** The findings that pass the state filter (`null`: all), each with its state. */
export function reviewRows(list: ReviewList, filter: string | null): readonly ReviewRow[] {
  const states = new Map(list.states.map((s) => [s.id, s]));
  return list.items.filter((f) => filter === null || f.state === filter).map((finding) => ({ finding, state: states.get(finding.state) ?? null }));
}

/** How many findings are in each state. */
export function reviewCounts(list: ReviewList): ReadonlyMap<string, number> {
  const out = new Map<string, number>();
  for (const f of list.items) out.set(f.state, (out.get(f.state) ?? 0) + 1);
  return out;
}

/**
 * A `ReviewList` node: findings with their location, detail and state (a labelled badge, never colour alone) and the actions the
 * plugin offers for that state (accept, dismiss, suppress ...), sent with `finding`. A state that needs a reason asks for it first, in
 * the row: the action is sent with `finding` and `reason` only when the reason is not empty. Buttons above filter by state.
 */
@Component({
  selector: 'pd-view-review',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [PdField, PdFieldControl, PdViewButton, PdViewLink, PdViewTone, PdViewWrites],
  template: `
    @if (list().states.length > 1) {
      <div class="pd-segmented pd-view-review__filter" role="group" [attr.aria-label]="filterName">
        <button type="button" [attr.aria-pressed]="filter() === null" (click)="filter.set(null)">{{ all }} ({{ list().items.length }})</button>
        @for (s of list().states; track s.id) {
          <button type="button" [attr.aria-pressed]="filter() === s.id" (click)="filter.set(s.id)">{{ s.label }} ({{ counts().get(s.id) ?? 0 }})</button>
        }
      </div>
    }
    <ul class="pd-view-review" [attr.aria-label]="label() || defaultLabel">
      @for (r of rows(); track r.finding.id) {
        <li class="pd-view-review__item" [class]="toneClass(r.finding.tone)" [attr.data-finding]="r.finding.id">
          <div class="pd-view-review__head">
            <pd-view-tone [tone]="r.finding.tone" />
            <span class="pd-view-review__title">{{ r.finding.title }}</span>
            @if (r.state; as s) {
              <span class="pd-view-review__state" [class]="toneClass(s.tone)" data-testid="view-finding-state"><pd-view-tone [tone]="s.tone" />{{ s.label }}</span>
            }
          </div>
          @if (r.finding.location) {
            <code class="pd-view-review__where">{{ r.finding.location }}</code>
          }
          @if (r.finding.detail) {
            <p class="pd-view-review__detail">{{ r.finding.detail }}</p>
          }
          <div class="pd-view-review__actions">
            @if (r.finding.link; as l) {
              <pd-view-link [link]="l" [extra]="{ finding: r.finding.id }" />
            }
            @if (asking()?.finding === r.finding.id) {
              @let q = asking()!;
              <div class="pd-view-review__reason">
                <pd-field [label]="reasonLabel(q.action)" [hint]="reasonHint" [state]="reasonState()" [submitted]="q.tried">
                  <textarea class="pd-textarea" pdFieldControl rows="2" [value]="q.reason" (input)="reason($any($event.target).value)" data-testid="view-reason"></textarea>
                </pd-field>
                <div class="pd-btn-row">
                  <button type="button" class="pd-btn" [class.pd-btn--primary]="runtime.primary() === null" [disabled]="runtime.disabled()" data-testid="view-reason-send" (click)="send()">
                    {{ q.action.label }}@if (q.action.writes) { <pd-view-writes /> }
                  </button>
                  <button type="button" class="pd-btn" data-testid="view-reason-cancel" (click)="asking.set(null)">{{ cancel }}</button>
                </div>
              </div>
            } @else if (r.state; as s) {
              @for (a of s.actions; track a.id) {
                @if (s.needsReason) {
                  <button type="button" class="pd-btn pd-btn--sm" [disabled]="runtime.disabled()" [attr.aria-label]="a.label + ': ' + r.finding.title" (click)="ask(r.finding.id, a)">
                    {{ a.label }}@if (a.writes) { <pd-view-writes /> }
                  </button>
                } @else {
                  <pd-view-button [action]="a" [extra]="{ finding: r.finding.id }" [small]="true" [context]="r.finding.title" />
                }
              }
            }
          </div>
        </li>
      } @empty {
        <li class="pd-view-review__none">{{ none }}</li>
      }
    </ul>
  `,
  styles: `
    ${TONE_CSS}
    :host { display: block; min-width: 0; }
    .pd-view-review__filter { flex-wrap: nowrap; max-width: 100%; overflow-x: auto; scrollbar-width: thin; margin-bottom: var(--pd-space-2); }
    .pd-view-review__filter > button { white-space: nowrap; }
    /* The filter scrolls on its own at a narrow width, which would cut a ring drawn outside the segment: draw it inside. */
    .pd-view-review__filter > button:focus-visible { outline-offset: -2px; }
    .pd-view-review { list-style: none; margin: 0; padding: 0; display: grid; grid-template-columns: minmax(0, 1fr); }
    .pd-view-review__item > * { min-width: 0; }
    .pd-view-review__item { display: grid; grid-template-columns: minmax(0, 1fr); min-width: 0; gap: var(--pd-space-1); padding: var(--pd-space-3); border-bottom: 1px solid var(--pd-hairline); border-inline-start: 3px solid var(--pd-view-tone); }
    .pd-view-review__item.pd-view-tone-neutral { border-inline-start-color: var(--pd-hairline); }
    .pd-view-review__head { display: flex; align-items: center; gap: var(--pd-space-2); flex-wrap: wrap; }
    .pd-view-review__title { font-weight: var(--pd-weight-semibold); min-width: 0; overflow-wrap: break-word; }
    .pd-view-review__state { display: inline-flex; align-items: center; gap: var(--pd-space-1); height: 20px; padding: 0 var(--pd-space-2); border-radius: var(--pd-radius-pill); background: var(--pd-view-tint); color: var(--pd-view-tone); font-size: var(--pd-text-label); font-weight: var(--pd-weight-medium); white-space: nowrap; }
    .pd-view-review__where { font-family: var(--pd-font-mono); font-size: var(--pd-text-label); color: var(--pd-text-muted); overflow-wrap: break-word; }
    .pd-view-review__detail { margin: 0; color: var(--pd-text-body); overflow-wrap: break-word; }
    .pd-view-review__actions { display: flex; flex-wrap: wrap; align-items: center; gap: var(--pd-space-2); }
    .pd-view-review__actions .pd-btn { max-width: 100%; white-space: normal; flex-wrap: wrap; }
    .pd-view-review__reason { display: grid; gap: var(--pd-space-2); width: 100%; }
    .pd-view-review__none { padding: var(--pd-space-4); text-align: center; color: var(--pd-text-muted); }
  `,
})
export class PdViewReview {
  protected readonly runtime = inject(ViewRuntime);
  readonly list = input.required<ReviewList>();
  readonly label = input('');
  protected readonly toneClass = toneClass;
  protected readonly toneOf = toneOf;
  protected readonly defaultLabel = $localize`:@@pd.view.review.label:Findings`;
  protected readonly filterName = $localize`:@@pd.view.review.filter:Show findings by state`;
  protected readonly all = $localize`:@@pd.view.review.all:All`;
  protected readonly none = $localize`:@@pd.view.review.none:No findings.`;
  protected readonly cancel = $localize`:@@pd.view.review.cancel:Cancel`;
  protected readonly reasonHint = $localize`:@@pd.view.review.reason-hint:Saved with the decision.`;
  protected readonly filter = signal<string | null>(null);
  protected readonly counts = computed(() => reviewCounts(this.list()));
  protected readonly rows = computed(() => reviewRows(this.list(), this.filter()));
  /** The finding being decided and the reason typed so far. */
  protected readonly asking = signal<{ finding: string; action: ActionRef; reason: string; tried: boolean } | null>(null);
  protected readonly reasonState = computed(() => {
    const a = this.asking();
    const empty = !a || a.reason.trim() === '';
    return { errors: () => (empty ? [{ kind: 'required' }] : []), touched: () => a?.tried === true, required: () => true };
  });

  protected reasonLabel(a: ActionRef): string {
    return $localize`:@@pd.view.review.reason:Reason for “${a.label}:action:”`;
  }
  protected ask(finding: string, action: ActionRef): void {
    this.asking.set({ finding, action, reason: '', tried: false });
  }
  protected reason(text: string): void {
    this.asking.update((a) => (a ? { ...a, reason: text } : a));
  }
  protected async send(): Promise<void> {
    const a = this.asking();
    if (!a) return;
    const reason = a.reason.trim();
    if (!reason) {
      this.asking.set({ ...a, tried: true });
      return;
    }
    // The confirmation (if the action has one) can be declined: the reason stays so nothing typed is lost.
    if (await this.runtime.run(a.action, { finding: a.finding, reason })) this.asking.set(null);
  }
}
