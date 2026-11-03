import { ChangeDetectionStrategy, Component, OnDestroy, OnInit, computed, inject } from '@angular/core';
import { Icon } from '../../shared/icon';
import { BaselinesStore } from '../../state/baselines.store';
import { itemName, normalLine, unusualItems, unusualStrength } from './normal-model';
import { MatTooltip } from '@angular/material/tooltip';

/**
 * "Is this normal?": what the engine finds unusual for this time of the week, with the current and the usual value. Calm by default:
 * while it learns there is one quiet line, and what is usual is never listed. Words carry the meaning, the dot only repeats it.
 */
@Component({
  selector: 'dy-normal-card',
  imports: [MatTooltip, Icon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @if (overview(); as o) {
      @if (visible()) {
        <section class="card" aria-label="Is this normal?">
          <div class="head">
            <dy-icon name="trend" />
            <h2>Is this normal?</h2>
            @if (line(); as l) {
              <span class="line">{{ l }}</span>
            }
          </div>
          @if (unusual().length) {
            <ul>
              @for (i of unusual(); track i.id) {
                <li [matTooltip]="i.judgement.explanation">
                  <span class="dot" [attr.data-tone]="tone(i)" aria-hidden="true"></span>
                  <span class="what"><strong>{{ name(i) }}:</strong> {{ i.judgement.phrase }}</span>
                  <span class="vals num">{{ i.current }} now · usually {{ i.judgement.usual }}</span>
                  <span class="visually-hidden">{{ i.judgement.explanation }}</span>
                </li>
              }
            </ul>
          }
        </section>
      }
    }
  `,
  styles: `
    :host { display: block; margin-bottom: var(--sp-3); }
    .card { padding: 0 var(--sp-4); }
    .head { display: flex; align-items: center; gap: var(--sp-3); min-height: 44px; }
    .head dy-icon { color: var(--text-3); }
    .head h2 { margin: 0; font-size: var(--fs-base); font-weight: var(--fw-semibold); }
    .line { color: var(--text-3); font-size: var(--fs-sm); min-width: 0; }
    ul { list-style: none; margin: 0; padding: 0 0 var(--sp-2); display: grid; border-top: 1px solid var(--border-subtle); }
    li { display: flex; flex-wrap: wrap; align-items: center; column-gap: var(--sp-3); row-gap: 0; min-height: 36px; padding-block: var(--sp-1); font-size: var(--fs-sm); border-top: 1px solid var(--border-subtle); }
    li:first-child { border-top: 0; }
    .what { flex: 1 1 16rem; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .what strong { font-weight: var(--fw-medium); }
    /* The values are never cut: when the row is too narrow they wrap under the sentence (which may ellipsize, its tooltip has the whole text). */
    .vals { margin-inline-start: auto; color: var(--text-3); white-space: nowrap; font-variant-numeric: tabular-nums; }
  `,
})
export class NormalCard implements OnInit, OnDestroy {
  protected readonly store = inject(BaselinesStore);
  protected readonly overview = this.store.overview;
  protected readonly unusual = computed(() => unusualItems(this.overview()?.items ?? []));
  protected readonly line = computed(() => {
    const o = this.overview();
    return o ? normalLine(o, this.unusual().length) : null;
  });
  /** Off means the user turned baselines off: the card is not shown at all. */
  protected readonly visible = computed(() => this.overview()?.learning !== 'off');
  protected readonly name = itemName;
  protected readonly tone = (i: Parameters<typeof unusualStrength>[0]) => (unusualStrength(i) === 'strong' ? 'warn' : 'info');

  ngOnInit(): void {
    void this.store.start();
  }

  ngOnDestroy(): void {
    this.store.stop();
  }
}
