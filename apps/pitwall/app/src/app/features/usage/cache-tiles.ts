import { ChangeDetectionStrategy, Component, computed, input } from '@angular/core';
import { MatTooltipModule } from '@angular/material/tooltip';
import type { UsageCounters } from '../../core/api.types';
import { compact, int } from '../../shared/format';
import { cacheHitRate, cacheHitRateText, tokensSaved } from './usage-format';

/** Two tiles over a usage range: how often the caches answered, and how many tokens they saved. */
@Component({
  selector: 'pw-cache-tiles',
  imports: [MatTooltipModule],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <section class="tile" aria-labelledby="ct-rate">
      <span id="ct-rate" class="label">Cache hit rate</span>
      <div class="value num" [matTooltip]="rateTip()">{{ rate() }}</div>
      <p class="sub faint">{{ rateSub() }}</p>
    </section>
    <section class="tile" aria-labelledby="ct-saved">
      <span id="ct-saved" class="label">Tokens saved</span>
      <div class="value num" [matTooltip]="savedTip()">{{ saved() }}</div>
      <p class="sub faint">{{ savedSub() }}</p>
    </section>
  `,
  styles: `
    :host {
      display: grid;
      grid-template-columns: repeat(2, minmax(0, 1fr));
      gap: var(--sp-3);
    }
    .tile {
      display: flex;
      flex-direction: column;
      gap: var(--sp-1);
      min-width: 0;
      padding: var(--sp-3) var(--sp-4);
      background: var(--bg-sunken);
      border: 1px solid var(--border-soft);
      border-radius: var(--r-md);
    }
    .label {
      font-size: var(--fs-sm);
      font-weight: var(--fw-medium);
      color: var(--text-2);
    }
    .value {
      font-size: var(--fs-2xl);
      font-weight: var(--fw-semibold);
      letter-spacing: -0.02em;
      line-height: 1.1;
    }
    .sub {
      margin: 0;
      font-size: var(--fs-sm);
    }
  `,
})
export class CacheTiles {
  readonly totals = input.required<UsageCounters>();

  protected readonly rate = computed(() => cacheHitRateText(this.totals()));
  protected readonly rateSub = computed(() => {
    const t = this.totals();
    return cacheHitRate(t) === null ? 'No cache-eligible requests yet' : `${int(t.cacheHits)} hits, ${int(t.cacheMisses)} misses`;
  });
  protected readonly rateTip = computed(() => 'Answers and embeddings served from the cache, out of the requests that could have been.');
  protected readonly saved = computed(() => {
    const t = this.totals();
    return tokensSaved(t) === 0 ? '—' : compact(tokensSaved(t));
  });
  protected readonly savedSub = computed(() => {
    const t = this.totals();
    return tokensSaved(t) === 0 ? 'Nothing reused yet' : `${compact(t.prefixTokens)} prompt reused, ${compact(t.savedTokens)} from cache hits`;
  });
  protected readonly savedTip = computed(() => {
    const t = this.totals();
    return `Prompt tokens reused: ${int(t.prefixTokens)}. Tokens skipped by cache hits: ${int(t.savedTokens)}.`;
  });
}
